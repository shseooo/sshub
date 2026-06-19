import { describe, it, expect } from 'vitest'
import {
  type KeyStore,
  type NewKey,
  listKeys,
  findKey,
  insertKey,
  updateKeyMeta,
  setPassphraseProtected,
  deleteKey,
} from './keyOps'
import type { SshKey } from '@/types/key'

const NOW = '2026-06-19T00:00:00.000Z'

function key(over: Partial<SshKey> = {}): SshKey {
  return {
    id: 1, name: 'k', publicKey: 'ssh-ed25519 AAAA', pemData: null, keyType: 'ed25519',
    keySize: 256, passphraseProtected: false, createdAt: NOW, ...over,
  }
}

const newKey: NewKey = {
  name: 'mykey', publicKey: 'ssh-rsa AAAA', keyType: 'rsa', keySize: 3072, passphraseProtected: true,
}

describe('insertKey', () => {
  it('assigns nextKeyId, increments, never stores pemData', () => {
    const { store, key: k } = insertKey({ keys: [], nextKeyId: 4 }, newKey, NOW)
    expect(k.id).toBe(4)
    expect(store.nextKeyId).toBe(5)
    expect(k.pemData).toBeNull()
    expect(k).toMatchObject({ name: 'mykey', keyType: 'rsa', keySize: 3072, passphraseProtected: true, createdAt: NOW })
  })

  it('does not mutate input', () => {
    const store: KeyStore = { keys: [], nextKeyId: 1 }
    insertKey(store, newKey, NOW)
    expect(store.keys).toHaveLength(0)
  })
})

describe('updateKeyMeta', () => {
  const store: KeyStore = { keys: [key({ id: 2, name: 'old', passphraseProtected: false })], nextKeyId: 3 }

  it('updates name/publicKey/keyType/passphraseProtected', () => {
    const { key: k } = updateKeyMeta(store, {
      id: 2, name: 'new', publicKey: 'ssh-rsa BBBB', keyType: 'rsa', passphraseProtected: true,
    })
    expect(k).toMatchObject({ name: 'new', publicKey: 'ssh-rsa BBBB', keyType: 'rsa', passphraseProtected: true })
  })

  it('throws when missing', () => {
    expect(() =>
      updateKeyMeta(store, { id: 99, name: 'x', publicKey: '', keyType: 'rsa', passphraseProtected: false })
    ).toThrow(/not found/i)
  })
})

describe('setPassphraseProtected', () => {
  it('flips the flag', () => {
    const store: KeyStore = { keys: [key({ id: 1, passphraseProtected: false })], nextKeyId: 2 }
    const next = setPassphraseProtected(store, 1, true)
    expect(next.keys[0].passphraseProtected).toBe(true)
  })
})

describe('deleteKey', () => {
  it('removes the matching key', () => {
    const store: KeyStore = { keys: [key({ id: 1 }), key({ id: 2 })], nextKeyId: 3 }
    expect(deleteKey(store, 1).keys.map((k) => k.id)).toEqual([2])
  })
})

describe('listKeys', () => {
  it('sorts case-insensitively by name without mutating input', () => {
    const keys = [key({ id: 1, name: 'Zed' }), key({ id: 2, name: 'alpha' })]
    expect(listKeys(keys).map((k) => k.name)).toEqual(['alpha', 'Zed'])
    expect(keys.map((k) => k.id)).toEqual([1, 2])
  })
})

describe('findKey', () => {
  it('returns the key or null', () => {
    const keys = [key({ id: 5 })]
    expect(findKey(keys, 5)?.id).toBe(5)
    expect(findKey(keys, 6)).toBeNull()
  })
})
