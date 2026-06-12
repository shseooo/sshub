import { createContext, useCallback, useContext, useState } from 'react'
import type { TerminalTab } from '@/types/terminal'

const uid = () => crypto.randomUUID()
const evenSizes = (n: number) => Array.from({ length: n }, () => 100 / n)

interface TerminalContextValue {
  tabs: TerminalTab[]
  activeTab: string | null
  setActiveTab: (tabId: string) => void
  openTab: (serverId: number | null, label: string) => void
  closeTab: (tabId: string) => void
  /** Add a pane to the active tab, splitting along `direction`. */
  splitActive: (direction: 'row' | 'column', serverId: number | null, label: string) => void
  /** Close one pane; closes the whole tab when it was the last pane. */
  closePane: (tabId: string, sessionId: string) => void
  setSizes: (tabId: string, sizes: number[]) => void
}

const TerminalContext = createContext<TerminalContextValue | null>(null)

// Holds terminal tab/pane state above the router so sessions survive route changes.
export function TerminalProvider({ children }: { children: React.ReactNode }) {
  const [tabs, setTabs] = useState<TerminalTab[]>([])
  const [activeTab, setActiveTab] = useState<string | null>(null)

  const openTab = useCallback((serverId: number | null, label: string) => {
    const tab: TerminalTab = {
      id: uid(),
      panes: [{ sessionId: uid(), serverId, label }],
      direction: 'row',
      sizes: [100],
    }
    setTabs((prev) => [...prev, tab])
    setActiveTab(tab.id)
  }, [])

  const closeTab = useCallback((tabId: string) => {
    setTabs((prev) => {
      const next = prev.filter((t) => t.id !== tabId)
      setActiveTab((cur) =>
        cur === tabId ? next[next.length - 1]?.id ?? null : cur
      )
      return next
    })
  }, [])

  const splitActive = useCallback(
    (direction: 'row' | 'column', serverId: number | null, label: string) => {
      setTabs((prev) =>
        prev.map((t) => {
          if (t.id !== activeTab) return t
          const panes = [...t.panes, { sessionId: uid(), serverId, label }]
          return {
            ...t,
            panes,
            // First split sets the axis; later splits keep it
            direction: t.panes.length === 1 ? direction : t.direction,
            sizes: evenSizes(panes.length),
          }
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
        const panes = t.panes.filter((p) => p.sessionId !== sessionId)
        if (panes.length === 0) continue // last pane → drop the tab
        next.push({ ...t, panes, sizes: evenSizes(panes.length) })
      }
      setActiveTab((cur) => (next.some((t) => t.id === cur) ? cur : next[next.length - 1]?.id ?? null))
      return next
    })
  }, [])

  const setSizes = useCallback((tabId: string, sizes: number[]) => {
    setTabs((prev) => prev.map((t) => (t.id === tabId ? { ...t, sizes } : t)))
  }, [])

  return (
    <TerminalContext.Provider
      value={{ tabs, activeTab, setActiveTab, openTab, closeTab, splitActive, closePane, setSizes }}
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
