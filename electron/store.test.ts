import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { normalizeData, Store } from './store'

describe('normalizeData', () => {
  it('fills defaults for an empty/absent file (ids start at 1)', () => {
    expect(normalizeData(null)).toEqual({ nextServerId: 1, nextKeyId: 1, servers: [], keys: [] })
  })

  it('keeps id counters ahead of existing records', () => {
    const d = normalizeData({
      nextServerId: 2,
      servers: [{ id: 3 }, { id: 7 }],
      keys: [{ id: 5 }],
    } as never)
    expect(d.nextServerId).toBe(8) // max(2, 7+1)
    expect(d.nextKeyId).toBe(6) // max(0, 5+1)
  })

  it('does not lower a counter that is already ahead', () => {
    const d = normalizeData({ nextServerId: 100, servers: [{ id: 3 }] } as never)
    expect(d.nextServerId).toBe(100)
  })

  it('scrubs any private key material (store must never hold secrets)', () => {
    const d = normalizeData({ keys: [{ id: 1, pemData: 'PRIVATE' }] } as never)
    expect(d.keys[0].pemData).toBeNull()
  })
})

describe('Store (file-backed, atomic, 0600)', () => {
  let dir: string
  let path: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'sshub-test-'))
    path = join(dir, 'sshub.json')
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true }))

  it('persists an inserted server and reloads it', () => {
    const s = new Store(path)
    s.load()
    const server = s.insertServer({ name: 'web', host: 'h', username: 'u', authType: 'key' })
    expect(server.id).toBe(1)

    const reloaded = new Store(path)
    reloaded.load()
    expect(reloaded.listServers().map((x) => x.name)).toEqual(['web'])
  })

  it('writes the file with 0600 permissions', () => {
    const s = new Store(path)
    s.load()
    s.insertServer({ name: 'web', host: 'h', username: 'u', authType: 'key' })
    expect(statSync(path).mode & 0o777).toBe(0o600)
  })

  it('scrubs secrets present in an existing file on load', () => {
    writeFileSync(path, JSON.stringify({ keys: [{ id: 1, pemData: 'LEAK' }] }))
    const s = new Store(path)
    s.load()
    expect(s.listKeysRaw()[0].pemData).toBeNull()
  })

  it('recovers from a corrupt file: boots empty, preserves the original, does not throw', () => {
    writeFileSync(path, '{ this is not valid json ')
    const s = new Store(path)
    expect(() => s.load()).not.toThrow()
    expect(s.listServers()).toEqual([])
    // a .corrupt.* backup of the unparseable file is kept for recovery
    const backups = readdirSync(dir).filter((f) => f.includes('.corrupt.'))
    expect(backups).toHaveLength(1)
    // and the store is now a valid, reloadable file
    const reloaded = new Store(path)
    reloaded.load()
    expect(reloaded.listServers()).toEqual([])
  })

  it('toggleFavorite flips and persists', () => {
    const s = new Store(path)
    s.load()
    const srv = s.insertServer({ name: 'web', host: 'h', username: 'u', authType: 'key' })
    s.toggleFavorite(srv.id)
    const reloaded = new Store(path)
    reloaded.load()
    expect(reloaded.findServer(srv.id)?.isFavorite).toBe(true)
  })
})
