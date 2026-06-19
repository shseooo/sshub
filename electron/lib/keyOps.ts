// Pure SSH-key metadata CRUD, ported from store.rs. No I/O — the command layer
// handles the 0600 key files (ssh-keygen, write/rename/delete). pemData is NEVER
// kept here (secrets live only in the ssh_keys/ files).

import type { SshKey, KeyType } from '@/types/key'

export interface KeyStore {
  keys: SshKey[]
  nextKeyId: number
}

export interface NewKey {
  name: string
  publicKey: string
  keyType: KeyType
  keySize: number
  passphraseProtected: boolean
}

export interface KeyMetaUpdate {
  id: number
  name: string
  publicKey: string
  keyType: KeyType
  passphraseProtected: boolean
}

export function listKeys(keys: SshKey[]): SshKey[] {
  return [...keys].sort((a, b) => {
    const an = a.name.toLowerCase()
    const bn = b.name.toLowerCase()
    return an < bn ? -1 : an > bn ? 1 : 0
  })
}

export function findKey(keys: SshKey[], id: number): SshKey | null {
  return keys.find((k) => k.id === id) ?? null
}

export function insertKey(store: KeyStore, nk: NewKey, now: string): { store: KeyStore; key: SshKey } {
  const key: SshKey = {
    id: store.nextKeyId,
    name: nk.name,
    publicKey: nk.publicKey,
    pemData: null, // secrets never live in the data
    keyType: nk.keyType,
    keySize: nk.keySize,
    passphraseProtected: nk.passphraseProtected,
    createdAt: now,
  }
  return { store: { keys: [...store.keys, key], nextKeyId: store.nextKeyId + 1 }, key }
}

export function updateKeyMeta(store: KeyStore, u: KeyMetaUpdate): { store: KeyStore; key: SshKey } {
  const idx = store.keys.findIndex((k) => k.id === u.id)
  if (idx === -1) throw new Error('SSH key not found')
  const key: SshKey = {
    ...store.keys[idx],
    name: u.name,
    publicKey: u.publicKey,
    keyType: u.keyType,
    passphraseProtected: u.passphraseProtected,
    pemData: null,
  }
  const keys = [...store.keys]
  keys[idx] = key
  return { store: { ...store, keys }, key }
}

export function setPassphraseProtected(store: KeyStore, id: number, protectedFlag: boolean): KeyStore {
  const keys = store.keys.map((k) => (k.id === id ? { ...k, passphraseProtected: protectedFlag } : k))
  return { ...store, keys }
}

export function deleteKey(store: KeyStore, id: number): KeyStore {
  return { ...store, keys: store.keys.filter((k) => k.id !== id) }
}
