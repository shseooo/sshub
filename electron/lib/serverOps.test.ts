import { describe, it, expect } from 'vitest'
import {
  type ServerStore,
  listServers,
  findServer,
  insertServer,
  updateServer,
  deleteServer,
  toggleFavorite,
} from './serverOps'
import type { Server, CreateServerDto } from '@/types/server'

const NOW = '2026-06-19T00:00:00.000Z'

function srv(over: Partial<Server> = {}): Server {
  return {
    id: 1,
    name: 'srv',
    host: 'h',
    port: 22,
    username: 'u',
    authType: 'key',
    keyId: null,
    pemData: null,
    proxyJump: null,
    groupName: null,
    tags: null,
    isFavorite: false,
    notes: null,
    lastConnectedAt: null,
    createdAt: NOW,
    updatedAt: NOW,
    ...over,
  }
}

const baseDto: CreateServerDto = { name: 'web', host: '1.2.3.4', username: 'root', authType: 'key' }

describe('insertServer', () => {
  it('assigns nextServerId and increments the counter', () => {
    const { store, server } = insertServer({ servers: [], nextServerId: 7 }, baseDto, NOW)
    expect(server.id).toBe(7)
    expect(store.nextServerId).toBe(8)
    expect(store.servers).toHaveLength(1)
  })

  it('defaults port to 22 when omitted, respects it when given', () => {
    expect(insertServer({ servers: [], nextServerId: 1 }, baseDto, NOW).server.port).toBe(22)
    expect(
      insertServer({ servers: [], nextServerId: 1 }, { ...baseDto, port: 2222 }, NOW).server.port
    ).toBe(2222)
  })

  it('never stores a PEM in the data (secrets live in 0600 files)', () => {
    const { server } = insertServer(
      { servers: [], nextServerId: 1 },
      { ...baseDto, authType: 'pem', pemData: 'PRIVATE KEY' },
      NOW
    )
    expect(server.pemData).toBeNull()
  })

  it('starts non-favorite with timestamps set and lastConnectedAt null', () => {
    const { server } = insertServer({ servers: [], nextServerId: 1 }, baseDto, NOW)
    expect(server.isFavorite).toBe(false)
    expect(server.createdAt).toBe(NOW)
    expect(server.updatedAt).toBe(NOW)
    expect(server.lastConnectedAt).toBeNull()
  })

  it('carries optional fields through', () => {
    const { server } = insertServer(
      { servers: [], nextServerId: 1 },
      { ...baseDto, keyId: 3, proxyJump: 'user@bastion', groupName: 'prod', tags: '["a"]', notes: 'n' },
      NOW
    )
    expect(server).toMatchObject({ keyId: 3, proxyJump: 'user@bastion', groupName: 'prod', tags: '["a"]', notes: 'n' })
  })

  it('does not mutate the input store', () => {
    const store: ServerStore = { servers: [], nextServerId: 1 }
    insertServer(store, baseDto, NOW)
    expect(store.servers).toHaveLength(0)
    expect(store.nextServerId).toBe(1)
  })
})

describe('updateServer', () => {
  const store: ServerStore = {
    servers: [srv({ id: 5, name: 'old', proxyJump: 'keep@me', groupName: 'g', notes: 'n' })],
    nextServerId: 6,
  }

  it('updates only provided fields and bumps updatedAt', () => {
    const { server } = updateServer(store, { id: 5, name: 'new', port: 2200 }, '2026-07-01T00:00:00.000Z')
    expect(server.name).toBe('new')
    expect(server.port).toBe(2200)
    expect(server.username).toBe('u') // untouched
    expect(server.updatedAt).toBe('2026-07-01T00:00:00.000Z')
  })

  it('treats proxyJump as authoritative — clears it when absent', () => {
    const { server } = updateServer(store, { id: 5, name: 'x' }, NOW)
    expect(server.proxyJump).toBeNull()
  })

  it('never persists a PEM', () => {
    const { server } = updateServer(store, { id: 5, authType: 'pem', pemData: 'SECRET' }, NOW)
    expect(server.pemData).toBeNull()
  })

  it('throws when the server is missing', () => {
    expect(() => updateServer(store, { id: 999, name: 'x' }, NOW)).toThrow(/not found/i)
  })

  it('does not mutate the input store', () => {
    updateServer(store, { id: 5, name: 'mutated?' }, NOW)
    expect(store.servers[0].name).toBe('old')
  })
})

describe('deleteServer', () => {
  it('removes the matching server, keeps the rest', () => {
    const store: ServerStore = { servers: [srv({ id: 1 }), srv({ id: 2 })], nextServerId: 3 }
    const next = deleteServer(store, 1)
    expect(next.servers.map((s) => s.id)).toEqual([2])
  })
})

describe('toggleFavorite', () => {
  it('flips the favorite flag', () => {
    const store: ServerStore = { servers: [srv({ id: 1, isFavorite: false })], nextServerId: 2 }
    const { server } = toggleFavorite(store, 1)
    expect(server.isFavorite).toBe(true)
  })

  it('throws when missing', () => {
    expect(() => toggleFavorite({ servers: [], nextServerId: 1 }, 1)).toThrow(/not found/i)
  })
})

describe('listServers', () => {
  it('sorts favorites first, then case-insensitive by name', () => {
    const servers = [
      srv({ id: 1, name: 'Zebra', isFavorite: false }),
      srv({ id: 2, name: 'alpha', isFavorite: false }),
      srv({ id: 3, name: 'beta', isFavorite: true }),
    ]
    expect(listServers(servers).map((s) => s.name)).toEqual(['beta', 'alpha', 'Zebra'])
  })

  it('does not mutate the input array', () => {
    const servers = [srv({ id: 1, name: 'b' }), srv({ id: 2, name: 'a' })]
    listServers(servers)
    expect(servers.map((s) => s.id)).toEqual([1, 2])
  })
})

describe('findServer', () => {
  it('returns the server or null', () => {
    const servers = [srv({ id: 1 })]
    expect(findServer(servers, 1)?.id).toBe(1)
    expect(findServer(servers, 2)).toBeNull()
  })
})
