import { Fragment, useEffect, useRef, useCallback, useState } from 'react'
import { useLocation, useSearchParams } from 'react-router-dom'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { listen } from '@tauri-apps/api/event'
import { X, Plus, SquareTerminal, Server, SplitSquareHorizontal, SplitSquareVertical, RotateCw, Radio } from 'lucide-react'
import '@xterm/xterm/css/xterm.css'
import { startTerminalSession, writeTerminal, resizeTerminal, closeTerminal } from '@/lib/tauriCommands'
import { useServers } from '@/hooks/useServers'
import { useTerminal, leaves } from '@/contexts/TerminalContext'
import { useT } from '@/contexts/LanguageContext'
import { useTheme } from '@/contexts/ThemeContext'
import { useShortcuts } from '@/contexts/ShortcutsContext'
import { comboFromEvent } from '@/lib/shortcuts'
import type { PaneNode, TerminalLeaf, TerminalTab } from '@/types/terminal'

const nodeKey = (n: PaneNode) => (n.type === 'leaf' ? n.sessionId : n.id)

interface Rect {
  sessionId: string
  x: number
  y: number
  w: number
  h: number
}

// Compute each leaf's normalized rectangle (0..1) from the split tree + sizes.
function computeRects(node: PaneNode, x: number, y: number, w: number, h: number, out: Rect[]) {
  if (node.type === 'leaf') {
    out.push({ sessionId: node.sessionId, x, y, w, h })
    return
  }
  let cursor = node.direction === 'row' ? x : y
  node.children.forEach((c, i) => {
    const frac = (node.sizes[i] ?? 100 / node.children.length) / 100
    if (node.direction === 'row') {
      const cw = w * frac
      computeRects(c, cursor, y, cw, h, out)
      cursor += cw
    } else {
      const ch = h * frac
      computeRects(c, x, cursor, w, ch, out)
      cursor += ch
    }
  })
}

// Nearest leaf in the given arrow direction from the focused leaf.
function pickInDirection(root: PaneNode, focused: string | null, arrow: string): string | null {
  const rects: Rect[] = []
  computeRects(root, 0, 0, 1, 1, rects)
  if (rects.length <= 1) return null
  const cur = rects.find((r) => r.sessionId === focused) ?? rects[0]
  const ccx = cur.x + cur.w / 2
  const ccy = cur.y + cur.h / 2
  const cand = rects
    .filter((r) => r.sessionId !== cur.sessionId)
    .map((r) => ({ id: r.sessionId, cx: r.x + r.w / 2, cy: r.y + r.h / 2 }))
  const horiz = arrow === 'ArrowLeft' || arrow === 'ArrowRight'
  const pool = cand.filter((c) =>
    arrow === 'ArrowRight'
      ? c.cx > ccx + 0.01
      : arrow === 'ArrowLeft'
        ? c.cx < ccx - 0.01
        : arrow === 'ArrowDown'
          ? c.cy > ccy + 0.01
          : c.cy < ccy - 0.01
  )
  if (pool.length === 0) return null
  pool.sort((a, b) => {
    const da = horiz ? Math.abs(a.cx - ccx) + Math.abs(a.cy - ccy) * 1.5 : Math.abs(a.cy - ccy) + Math.abs(a.cx - ccx) * 1.5
    const db = horiz ? Math.abs(b.cx - ccx) + Math.abs(b.cy - ccy) * 1.5 : Math.abs(b.cy - ccy) + Math.abs(b.cx - ccx) * 1.5
    return da - db
  })
  return pool[0].id
}

interface NodeCtx {
  tabId: string
  visible: boolean
  showHeader: boolean
  focusedPane: string | null
  onFocus: (sessionId: string) => void
  onClose: (sessionId: string) => void
  onReconnect: (sessionId: string) => void
  onResize: (splitId: string, sizes: number[]) => void
  /** Routes typed input — to this pane, or to all panes when broadcast is on. */
  onInput: (sessionId: string, data: string) => void
}

// One xterm per leaf. Stays mounted (hidden when its tab is inactive) so PTY
// sessions and scrollback survive tab switches. Refits via ResizeObserver.
function TerminalView({
  pane,
  visible,
  focused,
  onFocus,
  onInput,
}: {
  pane: TerminalLeaf
  visible: boolean
  focused: boolean
  onFocus: () => void
  onInput: (data: string) => void
}) {
  const { t } = useT()
  const { theme } = useTheme()
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<XTerm | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  // Keep the latest input router so the once-registered onData handler isn't stale
  // when broadcast mode toggles.
  const onInputRef = useRef(onInput)
  onInputRef.current = onInput

  const safeFit = useCallback(() => {
    const el = containerRef.current
    if (el && el.clientWidth > 0 && el.clientHeight > 0) fitRef.current?.fit()
  }, [])

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    const term = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: '"IBM Plex Mono", Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: theme.termBg,
        foreground: theme.termFg,
        cursor: theme.accent,
        cursorAccent: theme.termBg,
        selectionBackground: 'rgba(120, 120, 120, 0.35)',
      },
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.loadAddon(new WebLinksAddon())
    term.open(el)
    if (el.clientWidth > 0) fit.fit()
    term.focus()
    termRef.current = term
    fitRef.current = fit

    const dataDisp = term.onData((data) => {
      onInputRef.current(data)
    })
    const resizeDisp = term.onResize(({ cols, rows }) => {
      resizeTerminal(pane.sessionId, cols, rows).catch(() => {})
    })

    let cancelled = false
    const disposables: Array<() => void> = []

    ;(async () => {
      const unlistenOut = await listen<string>(`terminal-output-${pane.sessionId}`, (e) => term.write(e.payload))
      const unlistenClosed = await listen(`terminal-closed-${pane.sessionId}`, () => {
        term.write(`\r\n\x1b[90m[${t('term.closedNotice')}]\x1b[0m\r\n`)
      })
      disposables.push(unlistenOut, unlistenClosed)
      if (cancelled) return
      try {
        await startTerminalSession(pane.sessionId, pane.serverId)
        await resizeTerminal(pane.sessionId, term.cols, term.rows)
      } catch (err) {
        term.write(`\r\n\x1b[31m${t('term.connectFail')}: ${err}\x1b[0m\r\n`)
      }
    })()

    const ro = new ResizeObserver(() => safeFit())
    ro.observe(el)

    return () => {
      cancelled = true
      ro.disconnect()
      dataDisp.dispose()
      resizeDisp.dispose()
      disposables.forEach((d) => d())
      closeTerminal(pane.sessionId).catch(() => {})
      term.dispose()
    }
  }, [pane.sessionId, pane.serverId, safeFit])

  useEffect(() => {
    if (visible) {
      safeFit()
      termRef.current?.focus()
    }
  }, [visible, safeFit])

  // Move the real cursor when this pane becomes the focused one (e.g. via keyboard nav)
  useEffect(() => {
    if (focused && visible) termRef.current?.focus()
  }, [focused, visible])

  // Live-update colors when the theme changes (no session restart).
  useEffect(() => {
    const term = termRef.current
    if (!term) return
    term.options.theme = {
      background: theme.termBg,
      foreground: theme.termFg,
      cursor: theme.accent,
      cursorAccent: theme.termBg,
      selectionBackground: 'rgba(120, 120, 120, 0.35)',
    }
  }, [theme.termBg, theme.termFg, theme.accent])

  return <div ref={containerRef} className="h-full w-full" onMouseDown={onFocus} />
}

function LeafView({ leaf, ctx }: { leaf: TerminalLeaf; ctx: NodeCtx }) {
  const { t } = useT()
  return (
    <div className="relative h-full w-full min-w-0 min-h-0 overflow-hidden flex flex-col">
      {ctx.showHeader && (
        <div
          onMouseDown={() => ctx.onFocus(leaf.sessionId)}
          className={`flex items-center gap-2 px-2 h-6 text-[10px] border-b shrink-0 ${
            ctx.focusedPane === leaf.sessionId
              ? 'bg-accent text-phosphor border-phosphor/40'
              : 'bg-card text-muted-foreground border-border'
          }`}
        >
          <span className="truncate flex-1">{leaf.label}</span>
          <button
            onClick={(e) => {
              e.stopPropagation()
              ctx.onReconnect(leaf.sessionId)
            }}
            title={t('term.reconnect')}
            className="p-0.5 hover:text-phosphor"
          >
            <RotateCw className="h-3 w-3" />
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation()
              ctx.onClose(leaf.sessionId)
            }}
            title={t('common.close')}
            className="p-0.5 hover:text-destructive"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      )}
      <div className="flex-1 min-h-0">
        <TerminalView
          pane={leaf}
          visible={ctx.visible}
          focused={ctx.focusedPane === leaf.sessionId}
          onFocus={() => ctx.onFocus(leaf.sessionId)}
          onInput={(data) => ctx.onInput(leaf.sessionId, data)}
        />
      </div>
    </div>
  )
}

function SplitView({ split, ctx }: { split: Extract<PaneNode, { type: 'split' }>; ctx: NodeCtx }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const isRow = split.direction === 'row'

  const startDrag = (i: number, e: React.MouseEvent) => {
    e.preventDefault()
    const container = containerRef.current
    if (!container) return
    const total = isRow ? container.clientWidth : container.clientHeight
    const startPos = isRow ? e.clientX : e.clientY
    const start = [...split.sizes]
    const onMove = (ev: MouseEvent) => {
      const pos = isRow ? ev.clientX : ev.clientY
      let delta = ((pos - startPos) / total) * 100
      delta = Math.max(-(start[i] - 10), Math.min(start[i + 1] - 10, delta))
      const sizes = [...start]
      sizes[i] = start[i] + delta
      sizes[i + 1] = start[i + 1] - delta
      ctx.onResize(split.id, sizes)
    }
    const onUp = () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }

  return (
    <div ref={containerRef} className="h-full w-full flex" style={{ flexDirection: split.direction }}>
      {split.children.map((child, i) => (
        <Fragment key={nodeKey(child)}>
          <div
            className="relative min-w-0 min-h-0 overflow-hidden"
            style={{ flexBasis: `${split.sizes[i]}%`, flexGrow: 0, flexShrink: 0 }}
          >
            <PaneNodeView node={child} ctx={ctx} />
          </div>
          {i < split.children.length - 1 && (
            <div
              onMouseDown={(e) => startDrag(i, e)}
              className={`shrink-0 bg-border hover:bg-phosphor/60 transition-colors ${
                isRow ? 'w-1 cursor-col-resize' : 'h-1 cursor-row-resize'
              }`}
            />
          )}
        </Fragment>
      ))}
    </div>
  )
}

function PaneNodeView({ node, ctx }: { node: PaneNode; ctx: NodeCtx }) {
  return node.type === 'leaf' ? <LeafView leaf={node} ctx={ctx} /> : <SplitView split={node} ctx={ctx} />
}

export default function TerminalHost() {
  const { t } = useT()
  const { shortcuts } = useShortcuts()
  const location = useLocation()
  const visible = location.pathname === '/terminal'
  const [searchParams, setSearchParams] = useSearchParams()
  const { data: servers = [] } = useServers()
  const {
    tabs,
    activeTab,
    setActiveTab,
    openTab,
    closeTab,
    splitActive,
    closePane,
    reconnectTab,
    reconnectPane,
    setSplitSizes,
  } = useTerminal()
  const autoConnectedRef = useRef<string | null>(null)
  const [pickerMode, setPickerMode] = useState<'new' | 'row' | 'column' | null>(null)
  const [focusedPane, setFocusedPane] = useState<string | null>(null)
  const [activated, setActivated] = useState(false)
  // Broadcast: when on, typing in one pane is sent to every pane of the active tab.
  const [broadcast, setBroadcast] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  const current = tabs.find((tb) => tb.id === activeTab) ?? null

  useEffect(() => {
    if (visible) setActivated(true)
  }, [visible])

  const connectToServer = useCallback(
    (serverId: number) => {
      const server = servers.find((s) => s.id === serverId)
      if (!server) return
      openTab(serverId, `${server.name} - ${server.username}@${server.host}`)
    },
    [servers, openTab]
  )

  // Auto connect once when arriving at /terminal?serverId=
  useEffect(() => {
    if (!visible) return
    const serverId = searchParams.get('serverId')
    if (!serverId || servers.length === 0) return
    if (autoConnectedRef.current === serverId) return
    autoConnectedRef.current = serverId
    connectToServer(Number(serverId))
    setSearchParams({}, { replace: true })
  }, [visible, searchParams, servers, connectToServer, setSearchParams])

  // Keyboard shortcuts (only while the terminal page is visible)
  useEffect(() => {
    if (!visible) return
    const focusMove = (arrow: string, e: KeyboardEvent) => {
      if (!current) return
      e.preventDefault()
      const target = pickInDirection(current.root, focusedPane, arrow)
      if (target) setFocusedPane(target)
    }
    const onKey = (e: KeyboardEvent) => {
      // Tab switch: Cmd/Ctrl + 1..9 (fixed — a 1..9 family, not rebindable)
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
        const idx = Number(e.code.slice(5)) - 1
        if (tabs[idx]) {
          e.preventDefault()
          setActiveTab(tabs[idx].id)
        }
        return
      }
      const combo = comboFromEvent(e)
      if (combo === shortcuts.newTab) {
        e.preventDefault()
        openTab(null, t('term.local'))
      } else if (combo === shortcuts.closePane) {
        if (!current) return
        e.preventDefault()
        const ls = leaves(current.root)
        const target =
          focusedPane && ls.some((l) => l.sessionId === focusedPane)
            ? focusedPane
            : ls[ls.length - 1]?.sessionId
        if (target) closePane(current.id, target)
      } else if (combo === shortcuts.splitRight) {
        if (!current) return
        e.preventDefault()
        splitActive('row', null, t('term.local'), focusedPane)
      } else if (combo === shortcuts.splitDown) {
        if (!current) return
        e.preventDefault()
        splitActive('column', null, t('term.local'), focusedPane)
      } else if (combo === shortcuts.broadcast) {
        e.preventDefault()
        setBroadcast((b) => !b)
      } else if (combo === shortcuts.focusLeft) {
        focusMove('ArrowLeft', e)
      } else if (combo === shortcuts.focusRight) {
        focusMove('ArrowRight', e)
      } else if (combo === shortcuts.focusUp) {
        focusMove('ArrowUp', e)
      } else if (combo === shortcuts.focusDown) {
        focusMove('ArrowDown', e)
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [visible, current, focusedPane, tabs, shortcuts, openTab, closePane, splitActive, setActiveTab, t])

  // Close the picker on outside click
  useEffect(() => {
    if (!pickerMode) return
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setPickerMode(null)
    }
    window.addEventListener('mousedown', onClick)
    return () => window.removeEventListener('mousedown', onClick)
  }, [pickerMode])

  const labelFor = (serverId: number | null) => {
    if (serverId == null) return t('term.local')
    const s = servers.find((x) => x.id === serverId)
    return s ? `${s.name} - ${s.username}@${s.host}` : t('nav.servers')
  }
  const pick = (serverId: number | null) => {
    const label = labelFor(serverId)
    if (pickerMode === 'row' || pickerMode === 'column') splitActive(pickerMode, serverId, label, focusedPane)
    else openTab(serverId, label)
    setPickerMode(null)
  }
  const tabTitle = (tab: TerminalTab) => leaves(tab.root)[0]?.label ?? t('nav.terminal')
  const paneCount = (tab: TerminalTab) => leaves(tab.root).length

  return (
    <div className={visible ? 'flex flex-col h-full bg-background' : 'hidden'}>
      {/* Tab bar */}
      <div className="flex items-center h-10 bg-card border-b border-border">
        <div className="flex-1 flex items-center h-10 overflow-x-auto">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-3 h-full text-xs border-r border-border min-w-[130px] max-w-[220px] border-t-2 transition-colors ${
                activeTab === tab.id
                  ? 'bg-background text-phosphor border-t-[var(--phosphor)]'
                  : 'bg-card text-muted-foreground hover:bg-muted border-t-transparent'
              }`}
            >
              <span className={activeTab === tab.id ? 'led shrink-0' : 'led-off shrink-0'} />
              <span className="truncate">{tabTitle(tab)}</span>
              {paneCount(tab) > 1 && (
                <span className="text-[9px] text-muted-foreground/70 shrink-0">⊞{paneCount(tab)}</span>
              )}
              <span
                role="button"
                title={t('term.reconnect')}
                onClick={(e) => {
                  e.stopPropagation()
                  reconnectTab(tab.id)
                }}
                className="ml-auto p-0.5 hover:bg-secondary hover:text-phosphor shrink-0"
              >
                <RotateCw className="h-3 w-3" />
              </span>
              <span
                role="button"
                title={t('common.close')}
                onClick={(e) => {
                  e.stopPropagation()
                  closeTab(tab.id)
                }}
                className="p-0.5 hover:bg-secondary hover:text-destructive shrink-0"
              >
                <X className="h-3 w-3" />
              </span>
            </button>
          ))}

          <button
            onClick={() => openTab(null, t('term.local'))}
            title={t('term.newLocalTitle')}
            className="flex items-center px-3 h-full text-muted-foreground hover:text-phosphor hover:bg-muted transition-colors shrink-0"
          >
            <Plus className="h-3.5 w-3.5" />
          </button>
        </div>

        {/* Split + new-connection controls (share one picker) */}
        <div className="relative flex items-center shrink-0" ref={menuRef}>
          {current && paneCount(current) > 1 && (
            <button
              onClick={() => setBroadcast((b) => !b)}
              title={`${t('term.broadcast')} (⌘⇧I)`}
              className={`p-2 border-l border-border transition-colors ${
                broadcast ? 'text-phosphor bg-accent' : 'text-muted-foreground hover:text-phosphor hover:bg-muted'
              }`}
            >
              <Radio className="h-4 w-4" />
            </button>
          )}
          {current && (
            <div className="flex items-center border-l border-border">
              <button
                onClick={() => setPickerMode((m) => (m === 'row' ? null : 'row'))}
                title={t('term.splitRightTitle')}
                className={`p-2 hover:bg-muted transition-colors ${
                  pickerMode === 'row' ? 'text-phosphor bg-muted' : 'text-muted-foreground hover:text-phosphor'
                }`}
              >
                <SplitSquareHorizontal className="h-4 w-4" />
              </button>
              <button
                onClick={() => setPickerMode((m) => (m === 'column' ? null : 'column'))}
                title={t('term.splitDownTitle')}
                className={`p-2 hover:bg-muted transition-colors ${
                  pickerMode === 'column' ? 'text-phosphor bg-muted' : 'text-muted-foreground hover:text-phosphor'
                }`}
              >
                <SplitSquareVertical className="h-4 w-4" />
              </button>
            </div>
          )}

          <div className="px-2">
            <button
              onClick={() => setPickerMode((m) => (m === 'new' ? null : 'new'))}
              className={`flex items-center gap-1.5 px-3 py-1 text-xs border transition-colors ${
                pickerMode === 'new'
                  ? 'border-phosphor/60 text-phosphor'
                  : 'border-border text-muted-foreground hover:text-phosphor hover:border-phosphor/60'
              }`}
            >
              <Plus className="h-3 w-3" />
              {t('term.newConnection')}
            </button>
          </div>

          {pickerMode && (
            <div className="absolute right-2 top-full mt-1 w-64 max-h-80 overflow-y-auto bg-popover border border-border shadow-lg z-50 crt-in">
              <div className="px-3 py-1.5 text-[9px] tracking-[0.25em] uppercase text-phosphor/80 border-b border-border">
                {pickerMode === 'new'
                  ? t('term.pickNewTab')
                  : pickerMode === 'row'
                    ? t('term.pickSplitRight')
                    : t('term.pickSplitDown')}
              </div>
              <button
                onClick={() => pick(null)}
                className="w-full flex items-center gap-2 px-3 py-2 text-xs text-left hover:bg-accent hover:text-accent-foreground transition-colors"
              >
                <SquareTerminal className="h-3.5 w-3.5 text-phosphor shrink-0" />
                {t('term.local')}
              </button>
              <div className="px-3 py-1 text-[9px] tracking-[0.25em] uppercase text-muted-foreground/70 border-t border-border">
                Servers
              </div>
              {servers.length === 0 ? (
                <p className="px-3 py-2 text-xs text-muted-foreground/70">{t('term.noServers')}</p>
              ) : (
                servers.map((server) => (
                  <button
                    key={server.id}
                    onClick={() => pick(server.id)}
                    className="w-full flex items-center gap-2 px-3 py-2 text-xs text-left hover:bg-accent hover:text-accent-foreground transition-colors"
                  >
                    <Server className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                    <span className="min-w-0">
                      <span className="block truncate">{server.name}</span>
                      <span className="block truncate text-muted-foreground/70">
                        {server.username}@{server.host}
                      </span>
                    </span>
                  </button>
                ))
              )}
            </div>
          )}
        </div>
      </div>

      {/* Terminal area — every tab stays mounted; inactive ones are hidden.
          A phosphor top border signals broadcast (all-pane input) is armed. */}
      <div
        className={`flex-1 overflow-hidden relative ${
          broadcast ? 'border-t-2 border-phosphor' : ''
        }`}
      >
        {tabs.length === 0 && (
          <div className="flex items-center justify-center h-full">
            <div className="text-center crt-in">
              <pre className="font-display text-phosphor/80 glow-text text-2xl leading-tight mb-4 select-none">{`┌─[ sshub ]─┐
│  > ssh _  │
└───────────┘`}</pre>
              <p className="text-xs text-muted-foreground max-w-md mx-auto px-4">{t('term.emptyHint')}</p>
            </div>
          </div>
        )}
        {activated &&
          tabs.map((tab) => {
            const showHeader = leaves(tab.root).length > 1
            const ctx: NodeCtx = {
              tabId: tab.id,
              visible: tab.id === activeTab,
              showHeader,
              focusedPane,
              onFocus: setFocusedPane,
              onClose: (sid) => closePane(tab.id, sid),
              onReconnect: (sid) => reconnectPane(tab.id, sid),
              onResize: (splitId, sizes) => setSplitSizes(tab.id, splitId, sizes),
              onInput: (sid, data) => {
                if (broadcast && tab.id === activeTab) {
                  leaves(tab.root).forEach((l) => writeTerminal(l.sessionId, data).catch(() => {}))
                } else {
                  writeTerminal(sid, data).catch(() => {})
                }
              },
            }
            return (
              <div key={tab.id} className={tab.id === activeTab ? 'absolute inset-0' : 'hidden'}>
                <PaneNodeView node={tab.root} ctx={ctx} />
              </div>
            )
          })}
      </div>
    </div>
  )
}
