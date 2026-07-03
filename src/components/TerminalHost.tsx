import { Fragment, useEffect, useRef, useCallback, useState } from 'react'
import { useLocation, useSearchParams } from 'react-router-dom'
import { X, Plus, SquareTerminal, Server, SplitSquareHorizontal, SplitSquareVertical, RotateCw, Radio, ChevronUp, ChevronDown } from 'lucide-react'
import '@xterm/xterm/css/xterm.css'
import { writeTerminal } from '@/lib/commands'
import { pruneScrollback } from '@/lib/bridge'
import { TerminalPool } from '@/lib/terminalPool'
import { useServers } from '@/hooks/useServers'
import { useTerminal, leaves, type DropSide } from '@/contexts/TerminalContext'
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
  editingPane: string | null
  onFocus: (sessionId: string) => void
  onClose: (sessionId: string) => void
  onReconnect: (sessionId: string) => void
  onResize: (splitId: string, sizes: number[]) => void
  /** Persistent xterm pool — keeps PTY/scrollback alive across tab moves. */
  pool: TerminalPool
  onEditStart: (sessionId: string) => void
  onRename: (sessionId: string, label: string) => void
  /** Drag a pane onto a side of another to re-split (with live preview). */
  dragOverPane: string | null
  dragOverSide: DropSide | null
  onDragOverPane: (sessionId: string, side: DropSide) => void
  onDragEnd: () => void
  onMove: (srcId: string, dstId: string, side: DropSide) => void
  /** Merge a dragged tab's panes into this tab next to a target pane. */
  onMergeTab: (srcTabId: string, dstPaneId: string, side: DropSide) => void
  /** True when the tab being dragged is this very tab — can't merge into itself. */
  dragTabSelf: boolean
}

const TAB_DND_TYPE = 'application/x-sshub-tab'

// Double-click to rename; Enter/blur commits, Esc cancels.
function EditableLabel({
  value,
  editing,
  onStart,
  onCommit,
  className,
}: {
  value: string
  editing: boolean
  onStart: () => void
  onCommit: (next: string) => void
  className?: string
}) {
  const [draft, setDraft] = useState(value)
  useEffect(() => {
    if (editing) setDraft(value)
  }, [editing, value])

  if (!editing) {
    return (
      <span className={className} onDoubleClick={onStart} title={value}>
        {value}
      </span>
    )
  }
  return (
    <input
      autoFocus
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => onCommit(draft)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onCommit(draft)
        else if (e.key === 'Escape') onCommit(value)
      }}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      className="min-w-0 flex-1 bg-background border border-phosphor/50 px-1 text-xs text-foreground focus:outline-hidden"
    />
  )
}

const SIDE_STYLE: Record<DropSide, React.CSSProperties> = {
  left: { top: 0, left: 0, bottom: 0, width: '50%' },
  right: { top: 0, right: 0, bottom: 0, width: '50%' },
  top: { top: 0, left: 0, right: 0, height: '50%' },
  bottom: { bottom: 0, left: 0, right: 0, height: '50%' },
}

function nearestSide(x: number, y: number): DropSide {
  const d: Record<DropSide, number> = { left: x, right: 1 - x, top: y, bottom: 1 - y }
  return (Object.keys(d) as DropSide[]).reduce((a, b) => (d[a] <= d[b] ? a : b))
}

// Insertion boundary (0..count) for a drag at clientX over the tab strip:
// before the first tab whose horizontal midpoint the cursor hasn't passed.
function tabDropBoundary(strip: HTMLElement, clientX: number): number {
  const els = Array.from(strip.querySelectorAll<HTMLElement>('[data-tab-index]'))
  for (let i = 0; i < els.length; i++) {
    const r = els[i].getBoundingClientRect()
    if (clientX < r.left + r.width / 2) return i
  }
  return els.length
}

function LeafView({ leaf, ctx }: { leaf: TerminalLeaf; ctx: NodeCtx }) {
  const { t } = useT()
  const hostRef = useRef<HTMLDivElement>(null)
  const { pool, visible, focusedPane } = ctx

  // Reparent this session's persistent xterm into our host. On unmount we do
  // NOT dispose — the pool keeps it alive so the session survives moving between
  // tabs (merge/detach). It's disposed only when the session leaves the tree.
  // NOTE: do NOT focus here. On a cold-start restore every pane of the active
  // tab mounts at once, so an unconditional focus would let the LAST-mounted
  // pane win the keyboard focus while the highlight stays on the first pane
  // (focusedPane). The focus-on-focusedPane effect below is the single source of
  // truth, so highlight and cursor never desync.
  useEffect(() => {
    const el = hostRef.current
    if (!el) return
    pool.mountInto(leaf.sessionId, leaf.serverId, el)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [leaf.sessionId, leaf.serverId])

  useEffect(() => {
    if (visible) pool.refit(leaf.sessionId)
  }, [visible, leaf.sessionId, pool])

  useEffect(() => {
    if (visible && focusedPane === leaf.sessionId) pool.focus(leaf.sessionId)
  }, [visible, focusedPane, leaf.sessionId, pool])

  return (
    <div
      className="relative h-full w-full min-w-0 min-h-0 overflow-hidden flex flex-col"
      onDragOver={(e) => {
        // Only our pane/tab drags trigger the split overlay — never OS file drags.
        const types = Array.from(e.dataTransfer.types)
        const ours = types.includes('text/plain') || types.includes(TAB_DND_TYPE)
        if (!ours || ctx.editingPane != null || ctx.dragTabSelf) return
        e.preventDefault()
        const r = e.currentTarget.getBoundingClientRect()
        const side = nearestSide((e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height)
        ctx.onDragOverPane(leaf.sessionId, side)
      }}
      onDrop={(e) => {
        e.preventDefault()
        const side = ctx.dragOverSide
        if (!side) return
        const tabId = e.dataTransfer.getData(TAB_DND_TYPE)
        if (tabId) {
          ctx.onMergeTab(tabId, leaf.sessionId, side)
          return
        }
        const src = e.dataTransfer.getData('text/plain')
        if (src) ctx.onMove(src, leaf.sessionId, side)
      }}
    >
      {ctx.showHeader && (
        <div
          onMouseDown={() => ctx.onFocus(leaf.sessionId)}
          draggable={ctx.editingPane !== leaf.sessionId}
          onDragStart={(e) => e.dataTransfer.setData('text/plain', leaf.sessionId)}
          onDragEnd={ctx.onDragEnd}
          className={`flex items-center gap-2 px-2 h-6 text-[10px] border-b shrink-0 cursor-grab active:cursor-grabbing ${
            ctx.focusedPane === leaf.sessionId
              ? 'bg-accent text-phosphor border-phosphor/40'
              : 'bg-card text-muted-foreground border-border'
          }`}
        >
          <EditableLabel
            value={leaf.label}
            editing={ctx.editingPane === leaf.sessionId}
            onStart={() => ctx.onEditStart(leaf.sessionId)}
            onCommit={(next) => ctx.onRename(leaf.sessionId, next)}
            className="truncate flex-1"
          />
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
      <div
        ref={hostRef}
        className="flex-1 min-h-0"
        onMouseDown={() => ctx.onFocus(leaf.sessionId)}
      />
      {/* Drop preview: highlights the half where the dragged pane will land */}
      {ctx.dragOverPane === leaf.sessionId && ctx.dragOverSide && (
        <div
          className="absolute pointer-events-none z-20 bg-phosphor/25 border-2 border-phosphor"
          style={SIDE_STYLE[ctx.dragOverSide]}
        />
      )}
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
    renameTab,
    renamePane,
    movePane,
    mergeTab,
    detachPane,
    reorderTab,
    closeOthers,
    closeToRight,
  } = useTerminal()
  const autoConnectedRef = useRef<string | null>(null)
  const wasVisibleRef = useRef(false)
  const [pickerMode, setPickerMode] = useState<'new' | 'row' | 'column' | null>(null)
  const [focusedPane, setFocusedPane] = useState<string | null>(null)
  const [editingTab, setEditingTab] = useState<string | null>(null)
  const [editingPane, setEditingPane] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState<{ pane: string; side: DropSide } | null>(null)
  const [draggingTab, setDraggingTab] = useState<string | null>(null)
  // Insertion boundary (0..tabs.length) while a tab or pane is dragged over the
  // tab bar — drives the drop indicator, reorder, and detach-at-position.
  const [tabDropIndex, setTabDropIndex] = useState<number | null>(null)
  const tabStripRef = useRef<HTMLDivElement>(null)
  // Right-click tab menu (close / close others / close to right).
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number; tabId: string } | null>(null)
  const [activated, setActivated] = useState(false)
  // Broadcast: when on, typing in one pane is sent to every pane of the active tab.
  const [broadcast, setBroadcast] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  // The pane search is locked to (captured when the bar opens) so it doesn't
  // drift if focus moves to another pane mid-search.
  const [searchPane, setSearchPane] = useState<string | null>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const { theme, setTheme } = useTheme()

  const current = tabs.find((tb) => tb.id === activeTab) ?? null

  // The pane in-terminal search acts on: the focused pane, else the active tab's first.
  const searchTarget = (() => {
    if (!current) return null
    const ls = leaves(current.root)
    return (focusedPane && ls.some((l) => l.sessionId === focusedPane) ? focusedPane : ls[0]?.sessionId) ?? null
  })()

  const runSearch = (dir: 'next' | 'prev', q = searchQuery) => {
    if (!searchPane || !q) return
    if (dir === 'next') pool.searchNext(searchPane, q)
    else pool.searchPrevious(searchPane, q)
  }

  const closeSearch = () => {
    pool.clearAllSearch() // active pane may have changed mid-search → clear every pane
    setSearchOpen(false)
    const pane = searchPane
    setSearchPane(null)
    if (pane) pool.focus(pane)
  }

  // Cmd+F opens the search bar and (re)targets the currently active pane — so
  // after a split (focus moved to the new pane) pressing Cmd+F searches it.
  // While the bar stays open it's locked to that pane (focus drift won't move
  // it); pressing Cmd+F again retargets to whatever is active now.
  const openSearch = () => {
    const target = searchTarget
    if (searchPane && searchPane !== target) pool.clearSearch(searchPane) // drop old pane's highlights
    setSearchPane(target)
    setSearchOpen(true)
    if (target && searchQuery) pool.searchNext(target, searchQuery) // re-run on the new pane
    setTimeout(() => {
      searchInputRef.current?.focus()
      searchInputRef.current?.select()
    }, 0)
  }

  // Auto-close search if its locked pane leaves the view (pane closed / tab switched).
  useEffect(() => {
    if (!searchOpen) return
    const stillThere = !!current && leaves(current.root).some((l) => l.sessionId === searchPane)
    if (!stillThere) {
      pool.clearAllSearch()
      setSearchOpen(false)
      setSearchPane(null)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, tabs, searchOpen, searchPane])

  // Persistent xterm pool: terminals live here, not in the tab subtree, so a
  // session (PTY + scrollback) survives being moved between tabs (merge/detach).
  const poolRef = useRef<TerminalPool | null>(null)
  if (!poolRef.current) {
    poolRef.current = new TerminalPool({
      onInput: () => {},
      theme,
      closedNotice: t('term.closedNotice'),
      connectFail: t('term.connectFail'),
    })
  }
  const pool = poolRef.current

  // Keep the pool's input router and i18n strings current each render. Broadcast
  // fans typed input to every pane of the active tab.
  pool.cfg.onInput = (sid, data) => {
    if (broadcast) {
      const tab = tabs.find((tb) => leaves(tb.root).some((l) => l.sessionId === sid))
      if (tab && tab.id === activeTab) {
        leaves(tab.root).forEach((l) => writeTerminal(l.sessionId, data).catch(() => {}))
        return
      }
    }
    writeTerminal(sid, data).catch(() => {})
  }
  pool.cfg.closedNotice = t('term.closedNotice')
  pool.cfg.connectFail = t('term.connectFail')

  // Live theme update (no session restart).
  useEffect(() => {
    pool.setTheme(theme)
  }, [theme, pool])

  // Dispose sessions that left the tab tree (closed panes/tabs, reconnect swaps).
  useEffect(() => {
    const live = new Set(tabs.flatMap((tb) => leaves(tb.root)).map((l) => l.sessionId))
    pool.disposeExcept(live)
  }, [tabs, pool])

  // After any structural change (split/merge/detach/collapse), refit the active
  // tab's panes so each terminal fills its new cell.
  useEffect(() => {
    if (!current) return
    leaves(current.root).forEach((l) => pool.refit(l.sessionId))
  }, [tabs, activeTab, current, pool])

  // On tab switch/open, focus the active tab's terminal. openTab only sets
  // activeTab (not focusedPane), so without this a new/switched tab keeps focus
  // on the previous tab's hidden textarea and typing lands in the wrong session.
  useEffect(() => {
    if (!visible || !current) return
    const ls = leaves(current.root)
    const target = ls.find((l) => l.sessionId === focusedPane) ?? ls[0]
    if (!target) return
    setFocusedPane(target.sessionId)
    pool.focus(target.sessionId)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, visible])

  // On launch, drop saved scrollback for sessions no longer in the restored
  // layout (orphans), and flush live scrollback on app quit (best-effort; the
  // debounced save already keeps it within ~1.5s of the latest output).
  useEffect(() => {
    const liveIds = tabs.flatMap((tb) => leaves(tb.root)).map((l) => l.sessionId)
    pruneScrollback(liveIds).catch(() => {})
    const onUnload = () => pool.flushScrollback()
    window.addEventListener('beforeunload', onUnload)
    return () => window.removeEventListener('beforeunload', onUnload)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // NOTE: no disposeAll on unmount — TerminalHost only unmounts at app teardown
  // (process exit reaps the PTYs). A cleanup here would let React StrictMode's
  // mount→cleanup→mount cycle kill+respawn every session in dev.

  useEffect(() => {
    if (visible) setActivated(true)
  }, [visible])

  // Entering the terminal page with no tabs → open a default local terminal.
  useEffect(() => {
    if (visible && !wasVisibleRef.current) {
      if (!searchParams.get('serverId') && tabs.length === 0) openTab(null, t('term.local'))
    }
    wasVisibleRef.current = visible
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
    // No param (or we just cleared it) → reset the guard so connecting to the
    // SAME server again later still opens a fresh tab.
    if (!serverId) {
      autoConnectedRef.current = null
      return
    }
    if (servers.length === 0) return
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
      // In-terminal search: Cmd/Ctrl+F (fixed)
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.code === 'KeyF') {
        e.preventDefault()
        openSearch()
        return
      }
      // Tab switch: Cmd/Ctrl + 1..9 (fixed — a 1..9 family, not rebindable)
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
        const idx = Number(e.code.slice(5)) - 1
        if (tabs[idx]) {
          e.preventDefault()
          // Commit any in-flight IME composition to the current tab before the
          // focus moves, so a half-composed syllable isn't carried into the target.
          ;(document.activeElement as HTMLElement | null)?.blur()
          setActiveTab(tabs[idx].id)
        }
        return
      }
      const combo = comboFromEvent(e)
      if (combo === shortcuts.newTab) {
        e.preventDefault()
        ;(document.activeElement as HTMLElement | null)?.blur()
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
        setFocusedPane(splitActive('row', null, t('term.local'), focusedPane))
      } else if (combo === shortcuts.splitDown) {
        if (!current) return
        e.preventDefault()
        setFocusedPane(splitActive('column', null, t('term.local'), focusedPane))
      } else if (combo === shortcuts.broadcast) {
        e.preventDefault()
        setBroadcast((b) => !b)
      } else if (combo === shortcuts.fontIncrease) {
        e.preventDefault()
        setTheme({ termFontSize: Math.min(24, theme.termFontSize + 1) })
      } else if (combo === shortcuts.fontDecrease) {
        e.preventDefault()
        setTheme({ termFontSize: Math.max(10, theme.termFontSize - 1) })
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
  }, [visible, current, focusedPane, tabs, shortcuts, openTab, closePane, splitActive, setActiveTab, t, theme.termFontSize, setTheme])

  // Close the picker on outside click
  useEffect(() => {
    if (!pickerMode) return
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setPickerMode(null)
    }
    window.addEventListener('mousedown', onClick)
    return () => window.removeEventListener('mousedown', onClick)
  }, [pickerMode])

  // Dismiss the tab context menu on any outside click or key.
  useEffect(() => {
    if (!tabMenu) return
    const close = () => setTabMenu(null)
    window.addEventListener('mousedown', close)
    window.addEventListener('keydown', close)
    return () => {
      window.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', close)
    }
  }, [tabMenu])

  const labelFor = (serverId: number | null) => {
    if (serverId == null) return t('term.local')
    const s = servers.find((x) => x.id === serverId)
    return s ? `${s.name} - ${s.username}@${s.host}` : t('nav.servers')
  }
  const pick = (serverId: number | null) => {
    const label = labelFor(serverId)
    if (pickerMode === 'row' || pickerMode === 'column')
      setFocusedPane(splitActive(pickerMode, serverId, label, focusedPane))
    else openTab(serverId, label)
    setPickerMode(null)
  }
  const tabTitle = (tab: TerminalTab) => tab.name ?? leaves(tab.root)[0]?.label ?? t('nav.terminal')
  const paneCount = (tab: TerminalTab) => leaves(tab.root).length

  // Closing a tab with a remote session or multiple panes asks for confirmation.
  const doCloseTab = (tabId: string) => {
    const tab = tabs.find((tb) => tb.id === tabId)
    if (!tab) return
    const ls = leaves(tab.root)
    const risky = ls.length > 1 || ls.some((l) => l.serverId != null)
    if (!risky || window.confirm(t('term.confirmCloseTab'))) closeTab(tabId)
  }
  const doCloseOthers = (tabId: string) => {
    if (tabs.length > 1 && window.confirm(t('term.confirmCloseOthers'))) closeOthers(tabId)
    setTabMenu(null)
  }
  const doCloseRight = (tabId: string) => {
    const idx = tabs.findIndex((tb) => tb.id === tabId)
    if (idx >= 0 && idx < tabs.length - 1 && window.confirm(t('term.confirmCloseRight'))) closeToRight(tabId)
    setTabMenu(null)
  }

  return (
    <div className={visible ? 'flex flex-col h-full bg-background' : 'hidden'}>
      {/* Tab bar */}
      <div className="flex items-center h-10 bg-card border-b border-border">
        <div
          ref={tabStripRef}
          className={`flex-1 flex items-center h-10 overflow-x-auto transition-colors ${
            tabDropIndex !== null ? 'bg-accent/20' : ''
          }`}
          onDragOver={(e) => {
            // Accept tab drags (reorder) and pane-header drags (detach).
            const types = Array.from(e.dataTransfer.types)
            if (!types.includes(TAB_DND_TYPE) && !types.includes('text/plain')) return
            e.preventDefault()
            setTabDropIndex(tabDropBoundary(e.currentTarget, e.clientX))
          }}
          onDrop={(e) => {
            e.preventDefault()
            const idx = tabDropIndex ?? tabs.length
            // Dropping removes the dragged element, so its `dragend` may not fire
            // — clear all drag state here.
            setTabDropIndex(null)
            setDragOver(null)
            setDraggingTab(null)
            const tabId = e.dataTransfer.getData(TAB_DND_TYPE)
            if (tabId) {
              reorderTab(tabId, idx)
              return
            }
            const sid = e.dataTransfer.getData('text/plain')
            if (sid && activeTab) detachPane(activeTab, sid, idx)
          }}
        >
          {tabs.map((tab, i) => (
            <Fragment key={tab.id}>
              {tabDropIndex === i && <div className="w-0.5 self-stretch my-1 bg-phosphor shrink-0" />}
            <div
              role="button"
              tabIndex={0}
              data-tab-index={i}
              draggable={editingTab !== tab.id}
              onDragStart={(e) => {
                e.dataTransfer.setData(TAB_DND_TYPE, tab.id)
                e.dataTransfer.effectAllowed = 'move'
                setDraggingTab(tab.id)
              }}
              onDragEnd={() => {
                setDraggingTab(null)
                setDragOver(null)
                setTabDropIndex(null)
              }}
              onClick={() => setActiveTab(tab.id)}
              onDoubleClick={() => setEditingTab(tab.id)}
              onContextMenu={(e) => {
                e.preventDefault()
                setTabMenu({ x: e.clientX, y: e.clientY, tabId: tab.id })
              }}
              className={`flex items-center gap-2 px-3 h-full text-xs border-r border-border min-w-[130px] max-w-[220px] border-t-2 transition-colors cursor-pointer ${
                activeTab === tab.id
                  ? 'bg-background text-phosphor border-t-[var(--phosphor)]'
                  : 'bg-card text-muted-foreground hover:bg-muted border-t-transparent'
              }`}
            >
              <span className={activeTab === tab.id ? 'led shrink-0' : 'led-off shrink-0'} />
              <EditableLabel
                value={tabTitle(tab)}
                editing={editingTab === tab.id}
                onStart={() => setEditingTab(tab.id)}
                onCommit={(next) => {
                  renameTab(tab.id, next)
                  setEditingTab(null)
                }}
                className="truncate"
              />
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
                  doCloseTab(tab.id)
                }}
                className="p-0.5 hover:bg-secondary hover:text-destructive shrink-0"
              >
                <X className="h-3 w-3" />
              </span>
            </div>
            </Fragment>
          ))}
          {tabDropIndex === tabs.length && <div className="w-0.5 self-stretch my-1 bg-phosphor shrink-0" />}

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
        {searchOpen && (
          <div className="absolute top-2 right-3 z-50 flex items-center gap-1 bg-popover border border-border shadow-lg px-2 py-1 crt-in">
            <input
              ref={searchInputRef}
              autoFocus
              value={searchQuery}
              placeholder={t('term.searchPlaceholder')}
              onChange={(e) => {
                setSearchQuery(e.target.value)
                runSearch('next', e.target.value)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  runSearch(e.shiftKey ? 'prev' : 'next')
                } else if (e.key === 'Escape') {
                  e.preventDefault()
                  closeSearch()
                }
              }}
              className="bg-input text-foreground text-xs px-2 py-1 w-48 outline-none border border-border focus:border-phosphor"
            />
            <button
              onClick={() => runSearch('prev')}
              title={t('term.searchPrev')}
              className="p-1 text-muted-foreground hover:text-phosphor"
            >
              <ChevronUp className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => runSearch('next')}
              title={t('term.searchNext')}
              className="p-1 text-muted-foreground hover:text-phosphor"
            >
              <ChevronDown className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={closeSearch}
              title={t('common.close')}
              className="p-1 text-muted-foreground hover:text-destructive"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        )}
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
              editingPane,
              dragOverPane: dragOver?.pane ?? null,
              dragOverSide: dragOver?.side ?? null,
              onFocus: setFocusedPane,
              onClose: (sid) => closePane(tab.id, sid),
              onReconnect: (sid) => reconnectPane(tab.id, sid),
              onResize: (splitId, sizes) => setSplitSizes(tab.id, splitId, sizes),
              onEditStart: (sid) => setEditingPane(sid),
              onRename: (sid, label) => {
                renamePane(tab.id, sid, label)
                setEditingPane(null)
              },
              onDragOverPane: (sid, side) =>
                setDragOver((prev) =>
                  prev?.pane === sid && prev.side === side ? prev : { pane: sid, side }
                ),
              onDragEnd: () => {
                setDragOver(null)
                setTabDropIndex(null)
              },
              onMove: (src, dst, side) => {
                movePane(tab.id, src, dst, side)
                setDragOver(null)
              },
              onMergeTab: (srcTabId, dstPaneId, side) => {
                mergeTab(srcTabId, tab.id, dstPaneId, side)
                setDragOver(null)
                setDraggingTab(null)
              },
              dragTabSelf: draggingTab === tab.id,
              pool,
            }
            return (
              <div key={tab.id} className={tab.id === activeTab ? 'absolute inset-0' : 'hidden'}>
                <PaneNodeView node={tab.root} ctx={ctx} />
              </div>
            )
          })}
      </div>

      {/* Tab right-click menu */}
      {tabMenu && (
        <div
          className="fixed z-50 min-w-44 bg-popover border border-border shadow-lg crt-in py-1 text-xs"
          style={{ left: tabMenu.x, top: tabMenu.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <button
            onClick={() => {
              doCloseTab(tabMenu.tabId)
              setTabMenu(null)
            }}
            className="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-accent-foreground"
          >
            {t('term.closeTab')}
          </button>
          <button
            onClick={() => doCloseOthers(tabMenu.tabId)}
            disabled={tabs.length <= 1}
            className="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-accent-foreground disabled:opacity-40 disabled:hover:bg-transparent"
          >
            {t('term.closeOthers')}
          </button>
          <button
            onClick={() => doCloseRight(tabMenu.tabId)}
            disabled={tabs.findIndex((tb) => tb.id === tabMenu.tabId) >= tabs.length - 1}
            className="w-full text-left px-3 py-1.5 hover:bg-accent hover:text-accent-foreground disabled:opacity-40 disabled:hover:bg-transparent"
          >
            {t('term.closeRight')}
          </button>
        </div>
      )}
    </div>
  )
}
