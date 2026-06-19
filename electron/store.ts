// JSON-file-backed store, ported from store.rs. All data lives in a single
// file (the main process points it at ~/Library/Application Support/sshub.json,
// the same path the Tauri build used, so existing data is preserved). Every
// mutation is persisted atomically (temp file + rename) with 0600 permissions.

import { closeSync, fsyncSync, openSync, readFileSync, renameSync, writeSync, chmodSync, existsSync } from 'node:fs'
import type { Server, CreateServerDto, UpdateServerDto } from '@/types/server'
import type { SshKey } from '@/types/key'
import {
  type ServerStore,
  listServers,
  findServer,
  insertServer,
  updateServer,
  deleteServer,
  toggleFavorite,
} from './lib/serverOps'
import {
  type KeyStore,
  type NewKey,
  type KeyMetaUpdate,
  listKeys,
  insertKey,
  updateKeyMeta,
  setPassphraseProtected,
  deleteKey,
} from './lib/keyOps'
import {
  type ExportBundle,
  type ImportSummary,
  type ExportFilter,
  buildExportBundle,
  mergeBundle,
} from './lib/bundleOps'

export interface StoreData {
  nextServerId: number
  nextKeyId: number
  servers: Server[]
  keys: SshKey[]
}

/** Apply defaults, keep id counters ahead of records, and scrub any secrets. */
export function normalizeData(raw: Partial<StoreData> | null | undefined): StoreData {
  const servers = raw?.servers ?? []
  const keys = (raw?.keys ?? []).map((k) => ({ ...k, pemData: null }))
  const maxId = (xs: { id: number }[]) => xs.reduce((m, x) => Math.max(m, x.id), 0)
  return {
    nextServerId: Math.max(raw?.nextServerId ?? 0, maxId(servers) + 1),
    nextKeyId: Math.max(raw?.nextKeyId ?? 0, maxId(keys) + 1),
    servers,
    keys,
  }
}

function now(): string {
  return new Date().toISOString()
}

export class Store {
  private data: StoreData = { nextServerId: 1, nextKeyId: 1, servers: [], keys: [] }

  constructor(private readonly path: string) {}

  load(): void {
    let raw: Partial<StoreData> | null = null
    if (existsSync(this.path)) {
      raw = JSON.parse(readFileSync(this.path, 'utf8')) as Partial<StoreData>
    }
    const hadSecret = (raw?.keys ?? []).some((k) => k.pemData != null)
    this.data = normalizeData(raw)
    // One-time cleanup: if the file held key material, re-persist the scrubbed copy.
    if (hadSecret) this.save()
  }

  private save(): void {
    const tmp = `${this.path}.tmp`
    const json = JSON.stringify(this.data, null, 2)
    const fd = openSync(tmp, 'w', 0o600)
    try {
      writeSync(fd, json)
      fsyncSync(fd) // flush before rename so a crash can't truncate
    } finally {
      closeSync(fd)
    }
    renameSync(tmp, this.path)
    chmodSync(this.path, 0o600)
  }

  private slice(): ServerStore {
    return { servers: this.data.servers, nextServerId: this.data.nextServerId }
  }

  // ==================== Servers ====================

  listServers(): Server[] {
    return listServers(this.data.servers)
  }

  findServer(id: number): Server | null {
    return findServer(this.data.servers, id)
  }

  insertServer(dto: CreateServerDto): Server {
    const { store, server } = insertServer(this.slice(), dto, now())
    this.data.servers = store.servers
    this.data.nextServerId = store.nextServerId
    this.save()
    return server
  }

  updateServer(dto: UpdateServerDto): Server {
    const { store, server } = updateServer(this.slice(), dto, now())
    this.data.servers = store.servers
    this.save()
    return server
  }

  deleteServer(id: number): void {
    this.data.servers = deleteServer(this.slice(), id).servers
    this.save()
  }

  toggleFavorite(id: number): Server {
    const { store, server } = toggleFavorite(this.slice(), id)
    this.data.servers = store.servers
    this.save()
    return server
  }

  findKey(id: number): SshKey | null {
    return this.data.keys.find((k) => k.id === id) ?? null
  }

  // ==================== SSH Keys ====================

  private keySlice(): KeyStore {
    return { keys: this.data.keys, nextKeyId: this.data.nextKeyId }
  }

  listKeys(): SshKey[] {
    return listKeys(this.data.keys)
  }

  getKey(id: number): SshKey {
    const k = this.findKey(id)
    if (!k) throw new Error(`SSH key not found: ${id}`)
    return k
  }

  insertKey(nk: NewKey): SshKey {
    const { store, key } = insertKey(this.keySlice(), nk, now())
    this.data.keys = store.keys
    this.data.nextKeyId = store.nextKeyId
    this.save()
    return key
  }

  updateKeyMeta(u: KeyMetaUpdate): SshKey {
    const { store, key } = updateKeyMeta(this.keySlice(), u)
    this.data.keys = store.keys
    this.save()
    return key
  }

  setKeyPassphraseProtected(id: number, protectedFlag: boolean): void {
    this.data.keys = setPassphraseProtected(this.keySlice(), id, protectedFlag).keys
    this.save()
  }

  deleteKey(id: number): void {
    this.data.keys = deleteKey(this.keySlice(), id).keys
    this.save()
  }

  touchLastConnected(id: number): void {
    const s = this.data.servers.find((x) => x.id === id)
    if (s) {
      s.lastConnectedAt = now()
      this.save()
    }
  }

  // ==================== Export / Import ====================

  exportBundle(filter: ExportFilter = {}): ExportBundle {
    return buildExportBundle(this.data, filter)
  }

  importBundle(bundle: ExportBundle): ImportSummary {
    const { data, summary } = mergeBundle(this.data, bundle)
    this.data = data
    this.save()
    return summary
  }

  /** Test/inspection helper — the raw stored keys (secrets already scrubbed). */
  listKeysRaw(): SshKey[] {
    return this.data.keys
  }
}
