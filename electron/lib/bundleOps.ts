// Export/import bundle logic — ported from store.rs (export_bundle / import_bundle).
// Pure: builds a secret-free bundle and merges one in (skip-by-name, fresh ids).

import type { Server } from '@/types/server'
import type { SshKey } from '@/types/key'
import type { StoreData } from '../store'

export interface ExportBundle {
  version: number
  servers: Server[]
  keys: SshKey[]
  shortcuts?: Record<string, string> | null
}

export interface ImportSummary {
  serversAdded: number
  serversSkipped: number
  keysAdded: number
  keysSkipped: number
  shortcuts?: Record<string, string> | null
}

export interface ExportFilter {
  serverIds?: number[] | null
  keyIds?: number[] | null
  shortcuts?: Record<string, string> | null
}

/** Build a secret-free, optionally-filtered export bundle. */
export function buildExportBundle(data: StoreData, filter: ExportFilter = {}): ExportBundle {
  let servers = data.servers.map((s) => ({ ...s, pemData: null }))
  let keys = data.keys.map((k) => ({ ...k, pemData: null }))
  if (filter.serverIds) servers = servers.filter((s) => filter.serverIds!.includes(s.id))
  if (filter.keyIds) keys = keys.filter((k) => filter.keyIds!.includes(k.id))
  return { version: 1, servers, keys, shortcuts: filter.shortcuts ?? null }
}

/** Merge a bundle into the store data. Names that already exist are skipped
 *  (never overwritten); new entries get fresh ids and stripped secrets. */
export function mergeBundle(
  data: StoreData,
  bundle: ExportBundle
): { data: StoreData; summary: ImportSummary } {
  const summary: ImportSummary = {
    serversAdded: 0,
    serversSkipped: 0,
    keysAdded: 0,
    keysSkipped: 0,
    shortcuts: bundle.shortcuts ?? null,
  }

  const servers = [...data.servers]
  let nextServerId = data.nextServerId
  const serverNames = new Set(servers.map((s) => s.name))
  for (const s of bundle.servers) {
    if (serverNames.has(s.name)) {
      summary.serversSkipped++
      continue
    }
    serverNames.add(s.name)
    servers.push({ ...s, id: nextServerId++, pemData: null, keyId: null })
    summary.serversAdded++
  }

  const keys = [...data.keys]
  let nextKeyId = data.nextKeyId
  const keyNames = new Set(keys.map((k) => k.name))
  for (const k of bundle.keys) {
    if (keyNames.has(k.name)) {
      summary.keysSkipped++
      continue
    }
    keyNames.add(k.name)
    keys.push({ ...k, id: nextKeyId++, pemData: null })
    summary.keysAdded++
  }

  return { data: { servers, keys, nextServerId, nextKeyId }, summary }
}
