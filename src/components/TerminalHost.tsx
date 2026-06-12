import { Fragment, useEffect, useRef, useCallback, useState } from 'react'
import { useLocation, useSearchParams } from 'react-router-dom'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { listen } from '@tauri-apps/api/event'
import { X, Plus, SquareTerminal, Server, SplitSquareHorizontal, SplitSquareVertical } from 'lucide-react'
import '@xterm/xterm/css/xterm.css'
import {
  startTerminalSession,
  writeTerminal,
  resizeTerminal,
  closeTerminal,
} from '@/lib/tauriCommands'
import { useServers } from '@/hooks/useServers'
import { useTerminal } from '@/contexts/TerminalContext'
import { useT } from '@/contexts/LanguageContext'
import type { TerminalPane, TerminalTab } from '@/types/terminal'

// One xterm instance per pane. Stays mounted (hidden when its tab is inactive)
// so PTY sessions and scrollback survive tab switches. Refits via ResizeObserver,
// which covers pane resize, sidebar collapse, and window resize uniformly.
function TerminalView({
  pane,
  visible,
  onFocus,
}: {
  pane: TerminalPane
  visible: boolean
  onFocus: () => void
}) {
  const { t } = useT()
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<XTerm | null>(null)
  const fitRef = useRef<FitAddon | null>(null)

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
        background: '#0a0d0b',
        foreground: '#c2d4c4',
        cursor: '#3dff88',
        cursorAccent: '#04130a',
        selectionBackground: 'rgba(61, 255, 136, 0.25)',
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
      writeTerminal(pane.sessionId, data).catch(() => {})
    })
    const resizeDisp = term.onResize(({ cols, rows }) => {
      resizeTerminal(pane.sessionId, cols, rows).catch(() => {})
    })

    let cancelled = false
    const disposables: Array<() => void> = []

    ;(async () => {
      const unlistenOut = await listen<string>(`terminal-output-${pane.sessionId}`, (e) => {
        term.write(e.payload)
      })
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

  return <div ref={containerRef} className="h-full w-full" onMouseDown={onFocus} />
}

// Renders the panes of one tab in a resizable split layout.
function PaneLayout({
  tab,
  visible,
  focusedPane,
  onFocusPane,
  onClosePane,
}: {
  tab: TerminalTab
  visible: boolean
  focusedPane: string | null
  onFocusPane: (sessionId: string) => void
  onClosePane: (sessionId: string) => void
}) {
  const { setSizes } = useTerminal()
  const containerRef = useRef<HTMLDivElement>(null)
  const isRow = tab.direction === 'row'
  const multi = tab.panes.length > 1

  const startDrag = (i: number, e: React.MouseEvent) => {
    e.preventDefault()
    const container = containerRef.current
    if (!container) return
    const total = isRow ? container.clientWidth : container.clientHeight
    const startPos = isRow ? e.clientX : e.clientY
    const start = [...tab.sizes]

    const onMove = (ev: MouseEvent) => {
      const pos = isRow ? ev.clientX : ev.clientY
      let delta = ((pos - startPos) / total) * 100
      // keep both adjacent panes at >= 10%
      delta = Math.max(-(start[i] - 10), Math.min(start[i + 1] - 10, delta))
      const sizes = [...start]
      sizes[i] = start[i] + delta
      sizes[i + 1] = start[i + 1] - delta
      setSizes(tab.id, sizes)
    }
    const onUp = () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }

  return (
    <div
      ref={containerRef}
      className="h-full w-full flex"
      style={{ flexDirection: tab.direction }}
    >
      {tab.panes.map((pane, i) => (
        <Fragment key={pane.sessionId}>
          <div
            className="relative min-w-0 min-h-0 overflow-hidden flex flex-col"
            style={{ flexBasis: `${tab.sizes[i]}%`, flexGrow: 0, flexShrink: 0 }}
          >
            {multi && (
              <div
                onMouseDown={() => onFocusPane(pane.sessionId)}
                className={`flex items-center gap-2 px-2 h-6 text-[10px] border-b shrink-0 ${
                  focusedPane === pane.sessionId
                    ? 'bg-accent text-phosphor border-phosphor/40'
                    : 'bg-card text-muted-foreground border-border'
                }`}
              >
                <span className="truncate flex-1">{pane.label}</span>
                <button
                  onClick={(e) => {
                    e.stopPropagation()
                    onClosePane(pane.sessionId)
                  }}
                  className="p-0.5 hover:text-destructive"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            )}
            <div className="flex-1 min-h-0">
              <TerminalView
                pane={pane}
                visible={visible}
                onFocus={() => onFocusPane(pane.sessionId)}
              />
            </div>
          </div>

          {i < tab.panes.length - 1 && (
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

// Always mounted in App (outside Routes); only visible on /terminal.
export default function TerminalHost() {
  const { t } = useT()
  const location = useLocation()
  const visible = location.pathname === '/terminal'
  const [searchParams, setSearchParams] = useSearchParams()
  const { data: servers = [] } = useServers()
  const { tabs, activeTab, setActiveTab, openTab, closeTab, splitActive, closePane } = useTerminal()
  const autoConnectedRef = useRef<string | null>(null)
  // null = closed; 'new' opens a tab, 'row'/'column' split the active tab
  const [pickerMode, setPickerMode] = useState<'new' | 'row' | 'column' | null>(null)
  const [focusedPane, setFocusedPane] = useState<string | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  const current = tabs.find((t) => t.id === activeTab) ?? null

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

  // Keyboard: Cmd/Ctrl+T new local tab, Cmd/Ctrl+W close focused pane.
  useEffect(() => {
    if (!visible) return
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey
      if (!mod) return
      const key = e.key.toLowerCase()
      if (key === 't') {
        e.preventDefault()
        openTab(null, t('term.local'))
      } else if (key === 'w') {
        if (!current) return
        e.preventDefault()
        const target =
          focusedPane && current.panes.some((p) => p.sessionId === focusedPane)
            ? focusedPane
            : current.panes[current.panes.length - 1]?.sessionId
        if (target) closePane(current.id, target)
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [visible, current, focusedPane, openTab, closePane])

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
  // Apply the picker choice: new tab, or split the active tab along an axis.
  const pick = (serverId: number | null) => {
    const label = labelFor(serverId)
    if (pickerMode === 'row' || pickerMode === 'column') splitActive(pickerMode, serverId, label)
    else openTab(serverId, label)
    setPickerMode(null)
  }
  const tabTitle = (tab: TerminalTab) => tab.panes[0]?.label ?? t('nav.terminal')

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
              {tab.panes.length > 1 && (
                <span className="text-[9px] text-muted-foreground/70 shrink-0">
                  ⊞{tab.panes.length}
                </span>
              )}
              <span
                role="button"
                onClick={(e) => {
                  e.stopPropagation()
                  closeTab(tab.id)
                }}
                className="ml-auto p-0.5 hover:bg-secondary hover:text-destructive shrink-0"
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

      {/* Terminal area — every tab stays mounted; inactive ones are hidden */}
      <div className="flex-1 overflow-hidden relative">
        {tabs.length === 0 && (
          <div className="flex items-center justify-center h-full">
            <div className="text-center crt-in">
              <pre className="font-display text-phosphor/80 glow-text text-2xl leading-tight mb-4 select-none">{`┌─[ sshub ]─┐
│  > ssh _  │
└───────────┘`}</pre>
              <p className="text-xs text-muted-foreground max-w-md mx-auto px-4">
                {t('term.emptyHint')}
              </p>
            </div>
          </div>
        )}
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={tab.id === activeTab ? 'absolute inset-0' : 'hidden'}
          >
            <PaneLayout
              tab={tab}
              visible={tab.id === activeTab}
              focusedPane={focusedPane}
              onFocusPane={setFocusedPane}
              onClosePane={(sid) => closePane(tab.id, sid)}
            />
          </div>
        ))}
      </div>
    </div>
  )
}
