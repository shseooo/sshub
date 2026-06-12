export type ShortcutAction =
  | 'newTab'
  | 'closePane'
  | 'splitRight'
  | 'splitDown'
  | 'broadcast'
  | 'focusLeft'
  | 'focusRight'
  | 'focusUp'
  | 'focusDown'

export type Shortcuts = Record<ShortcutAction, string>

// Combo format: modifiers (ctrl/alt/shift/meta) + KeyboardEvent.code, joined by '+'.
// Modifier order MUST match comboFromEvent (ctrl, alt, shift, meta) so the
// stored default compares equal to the generated combo.
export const DEFAULT_SHORTCUTS: Shortcuts = {
  newTab: 'meta+KeyT',
  closePane: 'meta+KeyW',
  splitRight: 'meta+KeyD',
  splitDown: 'shift+meta+KeyD',
  broadcast: 'shift+meta+KeyI',
  focusLeft: 'alt+meta+ArrowLeft',
  focusRight: 'alt+meta+ArrowRight',
  focusUp: 'alt+meta+ArrowUp',
  focusDown: 'alt+meta+ArrowDown',
}

/** Order shown in the settings list, with i18n label keys. */
export const SHORTCUT_ACTIONS: { action: ShortcutAction; labelKey: string }[] = [
  { action: 'newTab', labelKey: 'shortcut.newTab' },
  { action: 'closePane', labelKey: 'shortcut.closePane' },
  { action: 'splitRight', labelKey: 'shortcut.splitRight' },
  { action: 'splitDown', labelKey: 'shortcut.splitDown' },
  { action: 'broadcast', labelKey: 'shortcut.broadcast' },
  { action: 'focusLeft', labelKey: 'shortcut.focusLeft' },
  { action: 'focusRight', labelKey: 'shortcut.focusRight' },
  { action: 'focusUp', labelKey: 'shortcut.focusUp' },
  { action: 'focusDown', labelKey: 'shortcut.focusDown' },
]

const MOD_ORDER = ['ctrl', 'alt', 'shift', 'meta']

/** Reorder modifiers into canonical order so combos compare equal regardless
 *  of the order they were written/stored in. */
export function normalizeCombo(combo: string): string {
  const parts = combo.split('+')
  const mods = parts.filter((p) => MOD_ORDER.includes(p)).sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b))
  const keys = parts.filter((p) => !MOD_ORDER.includes(p))
  return [...mods, ...keys].join('+')
}

/** Normalized combo string from a keyboard event (modifiers + physical key). */
export function comboFromEvent(e: KeyboardEvent | React.KeyboardEvent): string {
  const parts: string[] = []
  if (e.ctrlKey) parts.push('ctrl')
  if (e.altKey) parts.push('alt')
  if (e.shiftKey) parts.push('shift')
  if (e.metaKey) parts.push('meta')
  parts.push(e.code)
  return parts.join('+')
}

/** True if the combo is only modifiers (no real key yet) — used while capturing. */
export function isModifierOnly(code: string): boolean {
  return /^(Meta|Control|Shift|Alt)(Left|Right)$/.test(code)
}

const SYMBOLS: Record<string, string> = { meta: '⌘', ctrl: '⌃', alt: '⌥', shift: '⇧' }

/** Human-readable combo, e.g. "meta+shift+KeyD" → "⌘⇧D". */
export function formatCombo(combo: string): string {
  return combo
    .split('+')
    .map((p) => {
      if (SYMBOLS[p]) return SYMBOLS[p]
      if (p.startsWith('Key')) return p.slice(3)
      if (p.startsWith('Digit')) return p.slice(5)
      return p
    })
    .join('')
}

const KEY = 'shortcuts'

export function loadShortcuts(): Shortcuts {
  const result = { ...DEFAULT_SHORTCUTS }
  try {
    const raw = localStorage.getItem(KEY)
    if (raw) {
      const saved = JSON.parse(raw) as Partial<Shortcuts>
      // Normalize stored combos so old/mis-ordered values (e.g. "meta+shift+KeyD")
      // still match the canonical event order.
      for (const k of Object.keys(result) as (keyof Shortcuts)[]) {
        if (typeof saved[k] === 'string') result[k] = normalizeCombo(saved[k] as string)
      }
    }
  } catch {
    /* ignore */
  }
  return result
}

export function saveShortcuts(s: Shortcuts): void {
  localStorage.setItem(KEY, JSON.stringify(s))
}
