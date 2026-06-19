import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, mkdirSync, rmSync, statSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Store } from './store'
import { keyFileName } from './lib/keyFiles'
import * as keys from './keys'

let dir: string
let ctx: keys.KeyCtx
const mode = (p: string) => statSync(p).mode & 0o777
const keyPath = (name: string) => join(ctx.keysDir, keyFileName(name))

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'sshub-keys-'))
  const store = new Store(join(dir, 'sshub.json'))
  store.load()
  const keysDir = join(dir, 'ssh_keys')
  mkdirSync(keysDir, { recursive: true })
  ctx = { store, keysDir }
})
afterEach(() => rmSync(dir, { recursive: true, force: true }))

describe('importSshKey', () => {
  it('writes the private key as a 0600 file and never stores the secret', () => {
    const k = keys.importSshKey(ctx, {
      name: 'mykey',
      publicKey: 'ssh-ed25519 AAAA',
      pemData: 'PRIVATE-KEY-MATERIAL',
      keyType: 'ed25519',
    })
    expect(readFileSync(keyPath('mykey'), 'utf8')).toBe('PRIVATE-KEY-MATERIAL')
    expect(mode(keyPath('mykey'))).toBe(0o600)
    expect(ctx.store.findKey(k.id)?.pemData).toBeNull() // secret never in the store
  })

  it('detects key type from the public key', () => {
    const k = keys.importSshKey(ctx, { name: 'r', publicKey: 'ssh-rsa AAAA', keyType: 'ed25519' })
    expect(k.keyType).toBe('rsa')
  })

  it('requires at least a public or private key', () => {
    expect(() => keys.importSshKey(ctx, { name: 'x', publicKey: '', keyType: 'ed25519' })).toThrow()
  })
})

describe('updateSshKey rename', () => {
  it('moves the private key and its .pub when the name changes', () => {
    const k = keys.importSshKey(ctx, { name: 'old', publicKey: 'ssh-ed25519 A', pemData: 'PRIV', keyType: 'ed25519' })
    writeFileSync(`${keyPath('old')}.pub`, 'ssh-ed25519 A')
    keys.updateSshKey(ctx, { id: k.id, name: 'new', publicKey: 'ssh-ed25519 A', keyType: 'ed25519' })
    expect(existsSync(keyPath('old'))).toBe(false)
    expect(existsSync(keyPath('new'))).toBe(true)
    expect(existsSync(`${keyPath('new')}.pub`)).toBe(true)
    expect(ctx.store.findKey(k.id)?.name).toBe('new')
  })

  it('refuses to overwrite an existing key file on rename', () => {
    const a = keys.importSshKey(ctx, { name: 'a', publicKey: 'ssh-ed25519 A', pemData: 'P', keyType: 'ed25519' })
    keys.importSshKey(ctx, { name: 'b', publicKey: 'ssh-ed25519 B', pemData: 'P', keyType: 'ed25519' })
    expect(() => keys.updateSshKey(ctx, { id: a.id, name: 'b', publicKey: 'ssh-ed25519 A', keyType: 'ed25519' })).toThrow()
  })
})

describe('deleteSshKey', () => {
  it('removes the key files and the record', () => {
    const k = keys.importSshKey(ctx, { name: 'k', publicKey: 'ssh-ed25519 A', pemData: 'P', keyType: 'ed25519' })
    keys.deleteSshKey(ctx, k.id)
    expect(existsSync(keyPath('k'))).toBe(false)
    expect(ctx.store.findKey(k.id)).toBeNull()
  })
})

describe('getSshKeys', () => {
  it('reports whether each key has a private file on this machine', () => {
    keys.importSshKey(ctx, { name: 'withfile', publicKey: 'ssh-ed25519 A', pemData: 'P', keyType: 'ed25519' })
    keys.importSshKey(ctx, { name: 'nofile', publicKey: 'ssh-ed25519 B', keyType: 'ed25519' })
    const list = keys.getSshKeys(ctx)
    expect(list.find((k) => k.name === 'withfile')?.hasPrivateFile).toBe(true)
    expect(list.find((k) => k.name === 'nofile')?.hasPrivateFile).toBe(false)
  })
})

describe('createSshKey (real ssh-keygen)', () => {
  it('generates a 0600 private key + .pub and stores metadata (no secret)', () => {
    const k = keys.createSshKey(ctx, { name: 'gen', keyType: 'ed25519' })
    expect(existsSync(keyPath('gen'))).toBe(true)
    expect(mode(keyPath('gen'))).toBe(0o600)
    expect(existsSync(`${keyPath('gen')}.pub`)).toBe(true)
    expect(k.keyType).toBe('ed25519')
    expect(k.publicKey).toMatch(/^ssh-ed25519 /)
    expect(k.passphraseProtected).toBe(false)
    expect(ctx.store.findKey(k.id)?.pemData).toBeNull()
  })

  it('rejects a duplicate key file', () => {
    keys.createSshKey(ctx, { name: 'dup', keyType: 'ed25519' })
    expect(() => keys.createSshKey(ctx, { name: 'dup', keyType: 'ed25519' })).toThrow()
  })
})

describe('loadKeyFile', () => {
  it('detects a public key file', () => {
    const p = join(dir, 'k.pub')
    writeFileSync(p, 'ssh-ed25519 AAAA comment\n')
    const r = keys.loadKeyFile(p)
    expect(r.publicKey).toBe('ssh-ed25519 AAAA comment')
    expect(r.privateKey).toBeNull()
  })

  it('detects a private key file (BEGIN marker)', () => {
    const p = join(dir, 'id')
    writeFileSync(p, '-----BEGIN OPENSSH PRIVATE KEY-----\nx\n-----END OPENSSH PRIVATE KEY-----\n')
    const r = keys.loadKeyFile(p)
    expect(r.privateKey).toContain('BEGIN')
  })
})
