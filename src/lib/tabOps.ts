// Pure list-level operations on the tab array. Kept separate from the React
// context so they can be unit-tested without rendering. (Tree/pane operations
// live in TerminalContext; these only reorder/insert/filter whole tabs.)

/** Move `tabId` to the insertion boundary `toIndex` (0..length). */
export function reorderTabs<T extends { id: string }>(tabs: T[], tabId: string, toIndex: number): T[] {
  const from = tabs.findIndex((t) => t.id === tabId)
  if (from === -1) return tabs
  const arr = [...tabs]
  const [moved] = arr.splice(from, 1)
  // toIndex is a boundary over the original array; removing the dragged tab
  // shifts everything after it left by one.
  let idx = from < toIndex ? toIndex - 1 : toIndex
  idx = Math.max(0, Math.min(arr.length, idx))
  arr.splice(idx, 0, moved)
  return arr
}

/** Keep only `tabId` (close all others). */
export function tabsExcept<T extends { id: string }>(tabs: T[], tabId: string): T[] {
  return tabs.filter((t) => t.id === tabId)
}

/** Keep tabs up to and including `tabId` (close everything to its right). */
export function tabsUpToInclusive<T extends { id: string }>(tabs: T[], tabId: string): T[] {
  const idx = tabs.findIndex((t) => t.id === tabId)
  if (idx === -1) return tabs
  return tabs.slice(0, idx + 1)
}

/** Insert `item` at boundary `atIndex` (clamped); appends when `atIndex` is undefined. */
export function insertAtIndex<T>(arr: T[], item: T, atIndex?: number): T[] {
  const i = atIndex == null ? arr.length : Math.max(0, Math.min(arr.length, atIndex))
  const next = [...arr]
  next.splice(i, 0, item)
  return next
}
