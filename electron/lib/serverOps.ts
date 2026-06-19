// Pure server-CRUD logic, ported faithfully from the Rust store (store.rs).
// No I/O — operates on an in-memory { servers, nextServerId } slice and returns
// new state, so it's fully unit-testable. The store/IPC layer persists the result.
//
// Invariants kept from the Rust impl:
//  - ids come from a monotonic nextServerId counter
//  - port defaults to 22
//  - pemData is NEVER kept in the data (secrets live in 0600 files)
//  - proxyJump is authoritative on update (absent → cleared)
//  - listServers sorts favorites first, then case-insensitive by name

import type { Server, CreateServerDto, UpdateServerDto } from '@/types/server'

export interface ServerStore {
  servers: Server[]
  nextServerId: number
}

export function listServers(servers: Server[]): Server[] {
  return [...servers].sort((a, b) => {
    if (a.isFavorite !== b.isFavorite) return a.isFavorite ? -1 : 1
    const an = a.name.toLowerCase()
    const bn = b.name.toLowerCase()
    return an < bn ? -1 : an > bn ? 1 : 0
  })
}

export function findServer(servers: Server[], id: number): Server | null {
  return servers.find((s) => s.id === id) ?? null
}

export function insertServer(
  store: ServerStore,
  dto: CreateServerDto,
  now: string
): { store: ServerStore; server: Server } {
  const server: Server = {
    id: store.nextServerId,
    name: dto.name,
    host: dto.host,
    port: dto.port ?? 22,
    username: dto.username,
    authType: dto.authType,
    keyId: dto.keyId ?? null,
    pemData: null, // secrets never live in the data
    proxyJump: dto.proxyJump ?? null,
    groupName: dto.groupName ?? null,
    tags: dto.tags ?? null,
    isFavorite: false,
    notes: dto.notes ?? null,
    lastConnectedAt: null,
    createdAt: now,
    updatedAt: now,
  }
  return {
    store: { servers: [...store.servers, server], nextServerId: store.nextServerId + 1 },
    server,
  }
}

export function updateServer(
  store: ServerStore,
  dto: UpdateServerDto,
  now: string
): { store: ServerStore; server: Server } {
  const idx = store.servers.findIndex((s) => s.id === dto.id)
  if (idx === -1) throw new Error('Server not found')
  const prev = store.servers[idx]
  const server: Server = {
    ...prev,
    name: dto.name ?? prev.name,
    host: dto.host ?? prev.host,
    port: dto.port ?? prev.port,
    username: dto.username ?? prev.username,
    authType: dto.authType ?? prev.authType,
    keyId: dto.keyId !== undefined ? dto.keyId : prev.keyId,
    pemData: null, // never persisted here
    proxyJump: dto.proxyJump ?? null, // authoritative — absent clears it
    groupName: dto.groupName !== undefined ? dto.groupName : prev.groupName,
    tags: dto.tags !== undefined ? dto.tags : prev.tags,
    notes: dto.notes !== undefined ? dto.notes : prev.notes,
    updatedAt: now,
  }
  const servers = [...store.servers]
  servers[idx] = server
  return { store: { ...store, servers }, server }
}

export function deleteServer(store: ServerStore, id: number): ServerStore {
  return { ...store, servers: store.servers.filter((s) => s.id !== id) }
}

export function toggleFavorite(store: ServerStore, id: number): { store: ServerStore; server: Server } {
  const idx = store.servers.findIndex((s) => s.id === id)
  if (idx === -1) throw new Error('Server not found')
  const server = { ...store.servers[idx], isFavorite: !store.servers[idx].isFavorite }
  const servers = [...store.servers]
  servers[idx] = server
  return { store: { ...store, servers }, server }
}
