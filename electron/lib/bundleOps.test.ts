import { describe, it, expect } from 'vitest'
import { buildExportBundle, mergeBundle, type ExportBundle } from './bundleOps'
import type { StoreData } from '../store'
import type { Server } from '@/types/server'
import type { SshKey } from '@/types/key'

function srv(over: Partial<Server> = {}): Server {
  return {
    id: 1, name: 's', host: 'h', port: 22, username: 'u', authType: 'key', keyId: 9,
    pemData: 'SECRET', proxyJump: null, groupName: null, tags: null, isFavorite: false,
    notes: null, lastConnectedAt: null, createdAt: null, updatedAt: null, ...over,
  }
}
function key(over: Partial<SshKey> = {}): SshKey {
  return { id: 1, name: 'k', publicKey: 'p', pemData: 'SECRET', keyType: 'ed25519', keySize: 256, passphraseProtected: false, createdAt: null, ...over }
}
function data(over: Partial<StoreData> = {}): StoreData {
  return { nextServerId: 1, nextKeyId: 1, servers: [], keys: [], ...over }
}

describe('buildExportBundle', () => {
  it('strips secrets from servers and keys', () => {
    const b = buildExportBundle(data({ servers: [srv()], keys: [key()] }))
    expect(b.servers[0].pemData).toBeNull()
    expect(b.keys[0].pemData).toBeNull()
    expect(b.version).toBe(1)
  })

  it('filters by serverIds/keyIds when given', () => {
    const d = data({ servers: [srv({ id: 1 }), srv({ id: 2 })], keys: [key({ id: 1 }), key({ id: 2 })] })
    const b = buildExportBundle(d, { serverIds: [2], keyIds: [1] })
    expect(b.servers.map((s) => s.id)).toEqual([2])
    expect(b.keys.map((k) => k.id)).toEqual([1])
  })

  it('carries shortcuts', () => {
    expect(buildExportBundle(data(), { shortcuts: { a: 'b' } }).shortcuts).toEqual({ a: 'b' })
  })
})

describe('mergeBundle', () => {
  const bundle: ExportBundle = {
    version: 1,
    servers: [srv({ id: 99, name: 'new', keyId: 9, pemData: 'X' }), srv({ id: 5, name: 'dup' })],
    keys: [key({ id: 99, name: 'newkey', pemData: 'X' })],
    shortcuts: { x: 'y' },
  }
  const base = data({ servers: [srv({ id: 1, name: 'dup' })], keys: [], nextServerId: 2, nextKeyId: 1 })

  it('adds new entries with fresh ids, skips existing names', () => {
    const { data: d, summary } = mergeBundle(base, bundle)
    expect(summary).toMatchObject({ serversAdded: 1, serversSkipped: 1, keysAdded: 1, keysSkipped: 0 })
    const added = d.servers.find((s) => s.name === 'new')!
    expect(added.id).toBe(2) // fresh id from nextServerId
    expect(d.nextServerId).toBe(3)
  })

  it('strips secrets and clears dangling keyId on imported servers', () => {
    const added = mergeBundle(base, bundle).data.servers.find((s) => s.name === 'new')!
    expect(added.pemData).toBeNull()
    expect(added.keyId).toBeNull()
  })

  it('passes shortcuts through to the summary', () => {
    expect(mergeBundle(base, bundle).summary.shortcuts).toEqual({ x: 'y' })
  })

  it('does not mutate the input data', () => {
    mergeBundle(base, bundle)
    expect(base.servers).toHaveLength(1)
  })
})
