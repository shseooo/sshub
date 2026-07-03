import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { ScrollbackStore } from './scrollbackStore'
import { scrollbackFileName } from './lib/scrollback'

describe('ScrollbackStore', () => {
  let root: string
  let dir: string
  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), 'sshub-sb-'))
    dir = join(root, 'scrollback')
  })
  afterEach(() => rmSync(root, { recursive: true, force: true }))

  it('round-trips save/load per session and returns null for unknown ids', () => {
    const s = new ScrollbackStore(dir)
    expect(s.load('a')).toBeNull()
    s.save('a', 'line1\nline2')
    expect(s.load('a')).toBe('line1\nline2')
  })

  it('creates the directory 0700 and scrollback files 0600 (secrets may be on screen)', () => {
    const s = new ScrollbackStore(dir)
    expect(statSync(dir).mode & 0o777).toBe(0o700)
    s.save('sess', 'export TOKEN=secret')
    const file = join(dir, scrollbackFileName('sess'))
    expect(statSync(file).mode & 0o777).toBe(0o600)
  })

  it('deletes a single session file', () => {
    const s = new ScrollbackStore(dir)
    s.save('a', 'x')
    s.delete('a')
    expect(s.load('a')).toBeNull()
  })

  it('prunes files for sessions no longer in the layout', () => {
    const s = new ScrollbackStore(dir)
    s.save('keep', 'x')
    s.save('drop', 'y')
    s.prune(['keep'])
    expect(s.load('keep')).toBe('x')
    expect(s.load('drop')).toBeNull()
  })
})
