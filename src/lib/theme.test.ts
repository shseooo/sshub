import { afterEach, describe, expect, it } from 'vitest'
import { BG_PRESETS, DEFAULT_THEME, applyTheme, loadTheme, saveTheme } from './theme'

describe('applyTheme', () => {
  afterEach(() => document.documentElement.removeAttribute('style'))

  it('bakes the opacity alpha into --background', () => {
    applyTheme({ ...DEFAULT_THEME, bg: 'green', opacity: 20 })
    const bg = document.documentElement.style.getPropertyValue('--background')
    const [r, g, b] = [10, 13, 11] // BG_PRESETS.green.bg = #0a0d0b
    expect(bg).toBe(`rgba(${r}, ${g}, ${b}, 0.8)`)
  })

  it('is fully opaque at opacity 0', () => {
    applyTheme({ ...DEFAULT_THEME, opacity: 0 })
    expect(document.documentElement.style.getPropertyValue('--background')).toContain(', 1)')
  })

  it('sets the phosphor accent var from the theme', () => {
    applyTheme({ ...DEFAULT_THEME, accent: '#3dd6ff' })
    expect(document.documentElement.style.getPropertyValue('--phosphor')).toBe('#3dd6ff')
  })

  it('falls back to the green preset for an unknown bg', () => {
    // @ts-expect-error testing the runtime guard with a bad value
    applyTheme({ ...DEFAULT_THEME, bg: 'nope' })
    expect(document.documentElement.style.getPropertyValue('--card')).toBe(BG_PRESETS.green.card)
  })
})

describe('load/saveTheme', () => {
  afterEach(() => localStorage.clear())

  it('returns the default theme when nothing is stored', () => {
    expect(loadTheme()).toEqual(DEFAULT_THEME)
  })

  it('merges stored partial fields over the defaults', () => {
    localStorage.setItem('theme', JSON.stringify({ accent: '#ffb347' }))
    const t = loadTheme()
    expect(t.accent).toBe('#ffb347')
    expect(t.bg).toBe(DEFAULT_THEME.bg) // untouched fields keep defaults
  })

  it('round-trips through save', () => {
    const custom = { ...DEFAULT_THEME, opacity: 15, bg: 'warm' as const }
    saveTheme(custom)
    expect(loadTheme()).toEqual(custom)
  })
})
