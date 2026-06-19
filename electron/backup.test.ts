import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mkdtempSync, mkdirSync, rmSync, readFileSync, writeFileSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Store } from './store'
import * as backup from './backup'
import { keyFileName } from './lib/keyFiles'
import { isEncryptedEnvelope } from './lib/crypto'

function makeCtx(prefix: string): { dir: string; ctx: backup.BackupCtx } {
  const dir = mkdtempSync(join(tmpdir(), prefix))
  const store = new Store(join(dir, 'sshub.json'))
  store.load()
  const keysDir = join(dir, 'ssh_keys')
  mkdirSync(keysDir, { recursive: true })
  return { dir, ctx: { store, keysDir } }
}

let a: { dir: string; ctx: backup.BackupCtx }
let out: string
const cleanup: string[] = []
beforeEach(() => {
  a = makeCtx('sshub-bk-a-')
  out = join(a.dir, 'export')
})
afterEach(() => {
  rmSync(a.dir, { recursive: true, force: true })
  for (const d of cleanup.splice(0)) rmSync(d, { recursive: true, force: true })
})

describe('plain export/import', () => {
  it('writes a secret-free JSON export', () => {
    a.ctx.store.insertServer({ name: 's', host: 'h', username: 'u', authType: 'key' })
    a.ctx.store.insertKey({ name: 'k', publicKey: 'p', keyType: 'ed25519', keySize: 256, passphraseProtected: false })
    backup.exportData(a.ctx, out)
    const txt = readFileSync(out, 'utf8')
    expect(isEncryptedEnvelope(txt)).toBe(false)
    const j = JSON.parse(txt)
    expect(j.servers[0]).toMatchObject({ name: 's', pemData: null })
    expect(j.keys[0]).toMatchObject({ name: 'k', pemData: null })
  })

  it('merges a plain export, skipping names that already exist', () => {
    a.ctx.store.insertServer({ name: 's1', host: 'h', username: 'u', authType: 'key' })
    backup.exportData(a.ctx, out)
    const b = makeCtx('sshub-bk-b-')
    cleanup.push(b.dir)
    b.ctx.store.insertServer({ name: 's1', host: 'h2', username: 'u', authType: 'key' }) // duplicate name
    const sum = backup.importData(b.ctx, out)
    expect(sum.serversAdded).toBe(0)
    expect(sum.serversSkipped).toBe(1)
  })
})

describe('encrypted export/import', () => {
  it('encrypts, then restores metadata + the 0600 private key file', () => {
    a.ctx.store.insertKey({ name: 'mk', publicKey: 'ssh-ed25519 A', keyType: 'ed25519', keySize: 256, passphraseProtected: true })
    writeFileSync(join(a.ctx.keysDir, keyFileName('mk')), 'PRIVATE-KEY', { mode: 0o600 })

    backup.exportData(a.ctx, out, { passphrase: 'pw' })
    expect(isEncryptedEnvelope(readFileSync(out, 'utf8'))).toBe(true)

    const b = makeCtx('sshub-bk-b-')
    cleanup.push(b.dir)
    const sum = backup.importData(b.ctx, out, 'pw')
    expect(sum.keysAdded).toBe(1)
    const restored = join(b.ctx.keysDir, keyFileName('mk'))
    expect(readFileSync(restored, 'utf8')).toBe('PRIVATE-KEY')
    expect(statSync(restored).mode & 0o777).toBe(0o600)
  })

  it('rejects an encrypted file imported without a passphrase', () => {
    a.ctx.store.insertKey({ name: 'mk', publicKey: 'p', keyType: 'ed25519', keySize: 256, passphraseProtected: true })
    writeFileSync(join(a.ctx.keysDir, keyFileName('mk')), 'P', { mode: 0o600 })
    backup.exportData(a.ctx, out, { passphrase: 'pw' })
    const b = makeCtx('sshub-bk-b-')
    cleanup.push(b.dir)
    expect(() => backup.importData(b.ctx, out)).toThrow(/ENCRYPTED/)
  })

  it('rejects a wrong passphrase', () => {
    a.ctx.store.insertKey({ name: 'mk', publicKey: 'p', keyType: 'ed25519', keySize: 256, passphraseProtected: true })
    writeFileSync(join(a.ctx.keysDir, keyFileName('mk')), 'P', { mode: 0o600 })
    backup.exportData(a.ctx, out, { passphrase: 'right' })
    const b = makeCtx('sshub-bk-b-')
    cleanup.push(b.dir)
    expect(() => backup.importData(b.ctx, out, 'wrong')).toThrow(/복호화 실패/)
  })
})
