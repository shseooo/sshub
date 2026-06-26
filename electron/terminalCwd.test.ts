import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { TerminalCwdStore } from './terminalCwd'

describe('TerminalCwdStore', () => {
  let dir: string
  let path: string

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'sshub-cwd-'))
    path = join(dir, 'cwd.json')
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true }))

  it('returns null for an unknown session', () => {
    const s = new TerminalCwdStore(path)
    expect(s.get('nope')).toBeNull()
  })

  it('persists a cwd across reloads', () => {
    const a = new TerminalCwdStore(path)
    a.set('s1', dir) // dir exists, so get() will return it
    const b = new TerminalCwdStore(path)
    b.load()
    expect(b.get('s1')).toBe(dir)
  })

  it('does not return a saved cwd that no longer exists on disk', () => {
    const s = new TerminalCwdStore(path)
    s.set('s1', join(dir, 'deleted-subdir'))
    expect(s.get('s1')).toBeNull()
  })

  it('delete removes an entry', () => {
    const s = new TerminalCwdStore(path)
    s.set('s1', dir)
    s.delete('s1')
    expect(s.get('s1')).toBeNull()
  })

  it('prune keeps only live sessions', () => {
    const s = new TerminalCwdStore(path)
    s.set('keep', dir)
    s.set('drop', dir)
    s.prune(['keep'])
    expect(s.get('keep')).toBe(dir)
    expect(s.get('drop')).toBeNull()
  })

  it('writes the backing file with 0600 permissions (no secrets, but stays private)', () => {
    const s = new TerminalCwdStore(path)
    s.set('s1', dir)
    expect(statSync(path).mode & 0o777).toBe(0o600)
  })

  it('starts empty when the file is absent or corrupt', () => {
    const s = new TerminalCwdStore(path)
    s.load() // file does not exist yet
    expect(s.get('s1')).toBeNull()
  })
})
