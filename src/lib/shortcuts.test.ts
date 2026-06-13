import { afterEach, describe, expect, it } from 'vitest'
import {
  DEFAULT_SHORTCUTS,
  comboFromEvent,
  formatCombo,
  isModifierOnly,
  loadShortcuts,
  normalizeCombo,
  saveShortcuts,
} from './shortcuts'

describe('normalizeCombo', () => {
  it('reorders modifiers into canonical ctrl+alt+shift+meta order', () => {
    expect(normalizeCombo('meta+shift+KeyD')).toBe('shift+meta+KeyD')
    expect(normalizeCombo('meta+alt+ctrl+ArrowUp')).toBe('ctrl+alt+meta+ArrowUp')
  })

  it('leaves an already-canonical combo unchanged', () => {
    expect(normalizeCombo('shift+meta+KeyD')).toBe('shift+meta+KeyD')
  })

  it('keeps the key segment last even with no modifiers', () => {
    expect(normalizeCombo('KeyT')).toBe('KeyT')
  })
})

describe('comboFromEvent', () => {
  const ev = (init: KeyboardEventInit) => new KeyboardEvent('keydown', init)

  it('emits modifiers in canonical order matching the stored defaults', () => {
    // The Cmd+Shift+D regression: event order must equal DEFAULT_SHORTCUTS.splitDown.
    const combo = comboFromEvent(ev({ metaKey: true, shiftKey: true, code: 'KeyD' }))
    expect(combo).toBe('shift+meta+KeyD')
    expect(combo).toBe(DEFAULT_SHORTCUTS.splitDown)
  })

  it('includes the physical code for a bare key', () => {
    expect(comboFromEvent(ev({ metaKey: true, code: 'KeyT' }))).toBe('meta+KeyT')
  })
})

describe('formatCombo', () => {
  it('renders symbols and strips Key/Digit prefixes', () => {
    expect(formatCombo('shift+meta+KeyD')).toBe('⇧⌘D')
    expect(formatCombo('meta+Digit1')).toBe('⌘1')
    expect(formatCombo('alt+meta+ArrowLeft')).toBe('⌥⌘ArrowLeft')
  })
})

describe('isModifierOnly', () => {
  it('detects lone modifier keys', () => {
    expect(isModifierOnly('MetaLeft')).toBe(true)
    expect(isModifierOnly('ShiftRight')).toBe(true)
    expect(isModifierOnly('KeyA')).toBe(false)
  })
})

describe('load/saveShortcuts', () => {
  afterEach(() => localStorage.clear())

  it('falls back to defaults when nothing is stored', () => {
    expect(loadShortcuts()).toEqual(DEFAULT_SHORTCUTS)
  })

  it('normalizes mis-ordered stored combos on load', () => {
    saveShortcuts({ ...DEFAULT_SHORTCUTS, splitDown: 'meta+shift+KeyD' })
    expect(loadShortcuts().splitDown).toBe('shift+meta+KeyD')
  })

  it('ignores corrupt JSON and returns defaults', () => {
    localStorage.setItem('shortcuts', '{not json')
    expect(loadShortcuts()).toEqual(DEFAULT_SHORTCUTS)
  })
})
