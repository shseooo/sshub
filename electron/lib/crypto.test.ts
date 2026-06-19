import { describe, it, expect } from 'vitest'
import { encryptBundle, decryptBundle, isEncryptedEnvelope } from './crypto'

describe('encryptBundle / decryptBundle (AES-256-GCM + scrypt)', () => {
  it('round-trips plaintext with the right passphrase', () => {
    const env = encryptBundle('{"hello":"세계"}', 'hunter2')
    expect(decryptBundle(env, 'hunter2')).toBe('{"hello":"세계"}')
  })

  it('produces a recognizable encrypted envelope (not the plaintext)', () => {
    const env = encryptBundle('plaintext-secret', 'pw')
    expect(env).not.toContain('plaintext-secret')
    expect(isEncryptedEnvelope(env)).toBe(true)
  })

  it('uses a fresh salt/iv each time (different ciphertext for same input)', () => {
    expect(encryptBundle('same', 'pw')).not.toBe(encryptBundle('same', 'pw'))
  })

  it('throws on the wrong passphrase (GCM auth failure)', () => {
    const env = encryptBundle('secret', 'right')
    expect(() => decryptBundle(env, 'wrong')).toThrow(/복호화 실패/)
  })

  it('isEncryptedEnvelope is false for plain JSON / garbage', () => {
    expect(isEncryptedEnvelope('{"servers":[],"keys":[]}')).toBe(false)
    expect(isEncryptedEnvelope('not json')).toBe(false)
  })
})
