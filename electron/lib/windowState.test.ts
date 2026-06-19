import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { sanitizeBounds, loadWindowBounds, saveWindowBounds, type WindowBounds } from './windowState'

const DEFAULTS: WindowBounds = { width: 1000, height: 700 }

describe('sanitizeBounds', () => {
  it('falls back to defaults for non-object input', () => {
    expect(sanitizeBounds(null, DEFAULTS)).toEqual(DEFAULTS)
    expect(sanitizeBounds('nope', DEFAULTS)).toEqual(DEFAULTS)
  })

  it('keeps a valid saved size and position', () => {
    expect(sanitizeBounds({ x: 120, y: 80, width: 1280, height: 800 }, DEFAULTS)).toEqual({
      x: 120,
      y: 80,
      width: 1280,
      height: 800,
    })
  })

  it('rejects a too-small size and uses the defaults', () => {
    expect(sanitizeBounds({ width: 10, height: 10 }, DEFAULTS)).toEqual(DEFAULTS)
  })

  it('drops position when only one of x/y is present', () => {
    const r = sanitizeBounds({ x: 50, width: 1100, height: 720 }, DEFAULTS)
    expect(r).toEqual({ width: 1100, height: 720 })
  })

  it('rounds fractional device pixels', () => {
    expect(sanitizeBounds({ x: 10.6, y: 20.4, width: 1000.5, height: 700.5 }, DEFAULTS)).toEqual({
      x: 11,
      y: 20,
      width: 1001,
      height: 701,
    })
  })
})

describe('loadWindowBounds / saveWindowBounds', () => {
  let dir: string
  let path: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'sshub-win-'))
    path = join(dir, 'sshub_window.json')
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true }))

  it('returns defaults when the file is missing', () => {
    expect(loadWindowBounds(path, DEFAULTS)).toEqual(DEFAULTS)
  })

  it('returns defaults when the file is corrupt', () => {
    writeFileSync(path, '{ not json')
    expect(loadWindowBounds(path, DEFAULTS)).toEqual(DEFAULTS)
  })

  it('round-trips saved bounds through the file', () => {
    const b: WindowBounds = { x: 200, y: 150, width: 1366, height: 900 }
    saveWindowBounds(path, b)
    expect(loadWindowBounds(path, DEFAULTS)).toEqual(b)
  })
})
