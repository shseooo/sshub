import { createContext, useCallback, useContext, useEffect, useState } from 'react'
import type { PaneNode, TerminalLeaf, TerminalTab } from '@/types/terminal'

const uid = () => crypto.randomUUID()
const even = (n: number) => Array.from({ length: n }, () => 100 / n)
const newLeaf = (serverId: number | null, label: string): TerminalLeaf => ({
  type: 'leaf',
  sessionId: uid(),
  serverId,
  label,
})

// ==================== Pure tree operations ====================

export function leaves(node: PaneNode): TerminalLeaf[] {
  return node.type === 'leaf' ? [node] : node.children.flatMap(leaves)
}

// Split the target leaf along `direction`, adding `addition` after it.
// Same-direction parent → insert as a sibling (flat); otherwise nest.
function splitAt(node: PaneNode, sessionId: string, direction: 'row' | 'column', addition: TerminalLeaf): PaneNode {
  if (node.type === 'leaf') {
    if (node.sessionId !== sessionId) return node
    return { type: 'split', id: uid(), direction, children: [node, addition], sizes: [50, 50] }
  }
  const idx = node.children.findIndex((c) => c.type === 'leaf' && c.sessionId === sessionId)
  if (idx !== -1 && node.direction === direction) {
    const children = [...node.children]
    children.splice(idx + 1, 0, addition)
    return { ...node, children, sizes: even(children.length) }
  }
  return { ...node, children: node.children.map((c) => splitAt(c, sessionId, direction, addition)) }
}

function removeLeaf(node: PaneNode, sessionId: string): PaneNode | null {
  if (node.type === 'leaf') return node.sessionId === sessionId ? null : node
  const children = node.children
    .map((c) => removeLeaf(c, sessionId))
    .filter((c): c is PaneNode => c !== null)
  if (children.length === 0) return null
  if (children.length === 1) return children[0] // collapse single-child split
  return { ...node, children, sizes: even(children.length) }
}

function reconnectLeaf(node: PaneNode, sessionId: string): PaneNode {
  if (node.type === 'leaf') return node.sessionId === sessionId ? { ...node, sessionId: uid() } : node
  return { ...node, children: node.children.map((c) => reconnectLeaf(c, sessionId)) }
}

function reconnectAll(node: PaneNode): PaneNode {
  if (node.type === 'leaf') return { ...node, sessionId: uid() }
  return { ...node, children: node.children.map(reconnectAll) }
}

function setSizesAt(node: PaneNode, splitId: string, sizes: number[]): PaneNode {
  if (node.type === 'leaf') return node
  if (node.id === splitId) return { ...node, sizes }
  return { ...node, children: node.children.map((c) => setSizesAt(c, splitId, sizes)) }
}

// ==================== Persistence ====================

const LAYOUT_KEY = 'terminal-layout'

type SavedNode =
  | { type: 'leaf'; serverId: number | null; label: string }
  | { type: 'split'; direction: 'row' | 'column'; sizes: number[]; children: SavedNode[] }

function serializeNode(node: PaneNode): SavedNode {
  if (node.type === 'leaf') return { type: 'leaf', serverId: node.serverId, label: node.label }
  return { type: 'split', direction: node.direction, sizes: node.sizes, children: node.children.map(serializeNode) }
}

function reviveNode(s: SavedNode): PaneNode {
  if (s.type === 'leaf') return newLeaf(s.serverId, s.label)
  return { type: 'split', id: uid(), direction: s.direction, sizes: s.sizes, children: s.children.map(reviveNode) }
}

function loadLayout(): { tabs: TerminalTab[]; activeTab: string | null } {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY)
    if (!raw) return { tabs: [], activeTab: null }
    const saved = JSON.parse(raw) as { tabs: SavedNode[]; activeIndex: number }
    const tabs: TerminalTab[] = saved.tabs.map((root) => ({ id: uid(), root: reviveNode(root) }))
    const activeTab = tabs[saved.activeIndex]?.id ?? tabs[0]?.id ?? null
    return { tabs, activeTab }
  } catch {
    return { tabs: [], activeTab: null }
  }
}

// ==================== Context ====================

interface TerminalContextValue {
  tabs: TerminalTab[]
  activeTab: string | null
  setActiveTab: (tabId: string) => void
  openTab: (serverId: number | null, label: string) => void
  closeTab: (tabId: string) => void
  /** Split the focused pane of the active tab (or its first leaf). */
  splitActive: (direction: 'row' | 'column', serverId: number | null, label: string, focusedSession: string | null) => void
  /** Close one pane; closes the whole tab when it was the last pane. */
  closePane: (tabId: string, sessionId: string) => void
  reconnectTab: (tabId: string) => void
  reconnectPane: (tabId: string, sessionId: string) => void
  setSplitSizes: (tabId: string, splitId: string, sizes: number[]) => void
}

const TerminalContext = createContext<TerminalContextValue | null>(null)

export function TerminalProvider({ children }: { children: React.ReactNode }) {
  const initial = loadLayout()
  const [tabs, setTabs] = useState<TerminalTab[]>(initial.tabs)
  const [activeTab, setActiveTab] = useState<string | null>(initial.activeTab)

  useEffect(() => {
    const activeIndex = Math.max(0, tabs.findIndex((t) => t.id === activeTab))
    const data = { tabs: tabs.map((t) => serializeNode(t.root)), activeIndex }
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(data))
  }, [tabs, activeTab])

  const openTab = useCallback((serverId: number | null, label: string) => {
    const tab: TerminalTab = { id: uid(), root: newLeaf(serverId, label) }
    setTabs((prev) => [...prev, tab])
    setActiveTab(tab.id)
  }, [])

  const closeTab = useCallback((tabId: string) => {
    setTabs((prev) => {
      const next = prev.filter((t) => t.id !== tabId)
      setActiveTab((cur) => (cur === tabId ? next[next.length - 1]?.id ?? null : cur))
      return next
    })
  }, [])

  const splitActive = useCallback(
    (direction: 'row' | 'column', serverId: number | null, label: string, focusedSession: string | null) => {
      setTabs((prev) =>
        prev.map((t) => {
          if (t.id !== activeTab) return t
          // Split the focused pane if it's in this tab, else the first leaf.
          const all = leaves(t.root)
          const target = all.find((l) => l.sessionId === focusedSession) ?? all[0]
          if (!target) return t
          return { ...t, root: splitAt(t.root, target.sessionId, direction, newLeaf(serverId, label)) }
        })
      )
    },
    [activeTab]
  )

  const closePane = useCallback((tabId: string, sessionId: string) => {
    setTabs((prev) => {
      const next: TerminalTab[] = []
      for (const t of prev) {
        if (t.id !== tabId) {
          next.push(t)
          continue
        }
        const root = removeLeaf(t.root, sessionId)
        if (root) next.push({ ...t, root })
      }
      setActiveTab((cur) => (next.some((t) => t.id === cur) ? cur : next[next.length - 1]?.id ?? null))
      return next
    })
  }, [])

  const reconnectTab = useCallback((tabId: string) => {
    setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, root: reconnectAll(t.root) } : t)))
  }, [])

  const reconnectPane = useCallback((tabId: string, sessionId: string) => {
    setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, root: reconnectLeaf(t.root, sessionId) } : t)))
  }, [])

  const setSplitSizes = useCallback((tabId: string, splitId: string, sizes: number[]) => {
    setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, root: setSizesAt(t.root, splitId, sizes) } : t)))
  }, [])

  return (
    <TerminalContext.Provider
      value={{
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
      }}
    >
      {children}
    </TerminalContext.Provider>
  )
}

export function useTerminal() {
  const ctx = useContext(TerminalContext)
  if (!ctx) throw new Error('useTerminal must be used within TerminalProvider')
  return ctx
}
