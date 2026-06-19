import { readFileSync, writeFileSync } from 'node:fs'

// Persisted window geometry. x/y are optional so a fresh install (or corrupt
// file) lets the OS center the window with just a size.
export interface WindowBounds {
  x?: number
  y?: number
  width: number
  height: number
}

// Below these we treat the saved size as garbage and fall back to defaults, so
// a stray tiny value can't leave the app opening as an unusable sliver.
const MIN_W = 600
const MIN_H = 400

/**
 * Validate persisted bounds against the defaults. Bad/missing fields fall back
 * to defaults; x/y are kept only when both are present (a partial position is
 * meaningless). Numbers are rounded — fractional device pixels confuse some
 * window managers.
 */
export function sanitizeBounds(saved: unknown, defaults: WindowBounds): WindowBounds {
  if (!saved || typeof saved !== 'object') return { ...defaults }
  const s = saved as Record<string, unknown>
  const width = typeof s.width === 'number' && s.width >= MIN_W ? Math.round(s.width) : defaults.width
  const height = typeof s.height === 'number' && s.height >= MIN_H ? Math.round(s.height) : defaults.height
  const out: WindowBounds = { width, height }
  if (typeof s.x === 'number' && typeof s.y === 'number') {
    out.x = Math.round(s.x)
    out.y = Math.round(s.y)
  }
  return out
}

/** Load saved bounds, falling back to defaults on any read/parse failure. */
export function loadWindowBounds(path: string, defaults: WindowBounds): WindowBounds {
  try {
    return sanitizeBounds(JSON.parse(readFileSync(path, 'utf8')), defaults)
  } catch {
    return { ...defaults }
  }
}

/** Persist bounds (best-effort; geometry is not critical state). */
export function saveWindowBounds(path: string, bounds: WindowBounds): void {
  try {
    writeFileSync(path, JSON.stringify(bounds))
  } catch {
    /* best-effort: a failed geometry write must never block quit */
  }
}
