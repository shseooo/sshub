import { createContext, useCallback, useContext, useState } from 'react'
import {
  DEFAULT_SHORTCUTS,
  loadShortcuts,
  normalizeCombo,
  saveShortcuts,
  type ShortcutAction,
  type Shortcuts,
} from '@/lib/shortcuts'

interface ShortcutsContextValue {
  shortcuts: Shortcuts
  setShortcut: (action: ShortcutAction, combo: string) => void
  /** Apply an imported set (only known actions, normalized). */
  replaceShortcuts: (incoming: Record<string, string>) => void
}

const ShortcutsContext = createContext<ShortcutsContextValue | null>(null)

export function ShortcutsProvider({ children }: { children: React.ReactNode }) {
  const [shortcuts, setShortcuts] = useState<Shortcuts>(() => loadShortcuts())

  const setShortcut = useCallback((action: ShortcutAction, combo: string) => {
    setShortcuts((prev) => {
      const next = { ...prev, [action]: combo }
      saveShortcuts(next)
      return next
    })
  }, [])

  const replaceShortcuts = useCallback((incoming: Record<string, string>) => {
    setShortcuts((prev) => {
      const next = { ...prev }
      for (const action of Object.keys(DEFAULT_SHORTCUTS) as ShortcutAction[]) {
        if (typeof incoming[action] === 'string') next[action] = normalizeCombo(incoming[action])
      }
      saveShortcuts(next)
      return next
    })
  }, [])

  return (
    <ShortcutsContext.Provider value={{ shortcuts, setShortcut, replaceShortcuts }}>
      {children}
    </ShortcutsContext.Provider>
  )
}

export function useShortcuts() {
  const ctx = useContext(ShortcutsContext)
  if (!ctx) throw new Error('useShortcuts must be used within ShortcutsProvider')
  return ctx
}
