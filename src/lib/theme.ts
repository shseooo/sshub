export type BgPreset = 'green' | 'neutral' | 'warm' | 'black'

export interface Theme {
  /** Accent (phosphor) color, hex */
  accent: string
  /** Background tone preset */
  bg: BgPreset
  /** Terminal foreground/background, hex */
  termFg: string
  termBg: string
  /** UI translucency, 0 (opaque) .. 40 (%) */
  opacity: number
}

export const DEFAULT_THEME: Theme = {
  accent: '#3dff88',
  bg: 'green',
  termFg: '#c2d4c4',
  termBg: '#0a0d0b',
  opacity: 0,
}

export const ACCENT_PRESETS: { name: string; value: string }[] = [
  { name: 'green', value: '#3dff88' },
  { name: 'amber', value: '#ffb347' },
  { name: 'cyan', value: '#3dd6ff' },
  { name: 'magenta', value: '#ff5fd2' },
]

interface Surfaces {
  bg: string
  card: string
  popover: string
  muted: string
  accent: string
  secondary: string
  border: string
  input: string
  fg: string
  mutedFg: string
}

export const BG_PRESETS: Record<BgPreset, Surfaces> = {
  green: { bg: '#0a0d0b', card: '#0d120e', popover: '#10160f', muted: '#121913', accent: '#13251a', secondary: '#17211a', border: '#1c2a1f', input: '#243429', fg: '#c2d4c4', mutedFg: '#61805f' },
  neutral: { bg: '#0a0a0b', card: '#101012', popover: '#121214', muted: '#161618', accent: '#1c1c20', secondary: '#1a1a1d', border: '#242427', input: '#2c2c30', fg: '#cdced2', mutedFg: '#74757a' },
  warm: { bg: '#0d0b08', card: '#12100b', popover: '#16130d', muted: '#19150e', accent: '#251e10', secondary: '#211c12', border: '#2a2417', input: '#342c1c', fg: '#d8cdb8', mutedFg: '#84765a' },
  black: { bg: '#000000', card: '#0a0a0a', popover: '#0d0d0d', muted: '#111111', accent: '#161616', secondary: '#141414', border: '#202020', input: '#282828', fg: '#d0d0d0', mutedFg: '#707070' },
}

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '')
  const n = h.length === 3 ? h.split('').map((c) => c + c).join('') : h
  const int = parseInt(n, 16)
  return [(int >> 16) & 255, (int >> 8) & 255, int & 255]
}

function darken(hex: string, amt: number): string {
  const [r, g, b] = hexToRgb(hex)
  const f = (v: number) => Math.max(0, Math.round(v * (1 - amt)))
  return `rgb(${f(r)}, ${f(g)}, ${f(b)})`
}

/** Write the theme to CSS variables + body background (with translucency). */
export function applyTheme(t: Theme): void {
  const s = document.documentElement.style
  const p = BG_PRESETS[t.bg] ?? BG_PRESETS.green
  // Bake the translucency alpha into --background itself so every `bg-background`
  // surface (the full-screen app root included) becomes see-through. Cards/terminal
  // use their own opaque vars and stay solid.
  const alpha = 1 - Math.min(40, Math.max(0, t.opacity)) / 100
  const [br, bgc, bb] = hexToRgb(p.bg)
  s.setProperty('--background', `rgba(${br}, ${bgc}, ${bb}, ${alpha})`)
  s.setProperty('--card', p.card)
  s.setProperty('--popover', p.popover)
  s.setProperty('--muted', p.muted)
  s.setProperty('--accent', p.accent)
  s.setProperty('--secondary', p.secondary)
  s.setProperty('--border', p.border)
  s.setProperty('--input', p.input)
  s.setProperty('--foreground', p.fg)
  s.setProperty('--card-foreground', p.fg)
  s.setProperty('--popover-foreground', p.fg)
  s.setProperty('--secondary-foreground', p.fg)
  s.setProperty('--accent-foreground', p.fg)
  s.setProperty('--muted-foreground', p.mutedFg)

  const [r, g, b] = hexToRgb(t.accent)
  s.setProperty('--phosphor', t.accent)
  s.setProperty('--phosphor-dim', darken(t.accent, 0.14))
  s.setProperty('--phosphor-glow', `rgba(${r}, ${g}, ${b}, 0.4)`)
  s.setProperty('--phosphor-faint', `rgba(${r}, ${g}, ${b}, 0.07)`)
  s.setProperty('--primary', darken(t.accent, 0.14))
  s.setProperty('--ring', t.accent)

  // Body paints only the faint glow over a transparent base, so the single
  // translucent layer is --background (avoids alpha compounding).
  document.body.style.background =
    'radial-gradient(1100px 700px at 75% -10%, var(--phosphor-faint), transparent 60%)'
}

const KEY = 'theme'

export function loadTheme(): Theme {
  try {
    const raw = localStorage.getItem(KEY)
    if (raw) return { ...DEFAULT_THEME, ...JSON.parse(raw) }
  } catch {
    /* ignore */
  }
  return { ...DEFAULT_THEME }
}

export function saveTheme(t: Theme): void {
  localStorage.setItem(KEY, JSON.stringify(t))
}
