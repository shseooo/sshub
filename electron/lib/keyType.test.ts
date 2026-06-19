import { describe, it, expect } from 'vitest'
import { detectKeyType, defaultKeySize, normalizeCreatableKeyType } from './keyType'

describe('detectKeyType (ported from key.rs)', () => {
  it('maps known prefixes', () => {
    expect(detectKeyType('ssh-ed25519 AAAA x')).toBe('ed25519')
    expect(detectKeyType('ssh-rsa AAAA x')).toBe('rsa')
    expect(detectKeyType('ssh-dss AAAA x')).toBe('dsa')
    expect(detectKeyType('ecdsa-sha2-nistp256 AAAA x')).toBe('ecdsa')
  })

  it('handles FIDO2 / security-key prefixes', () => {
    expect(detectKeyType('sk-ssh-ed25519@openssh.com AAAA x')).toBe('ed25519')
    expect(detectKeyType('sk-ecdsa-sha2-nistp256@openssh.com AAAA x')).toBe('ecdsa')
  })

  it('returns null for unknown/empty', () => {
    expect(detectKeyType('not-a-key')).toBeNull()
    expect(detectKeyType('')).toBeNull()
    expect(detectKeyType('   ')).toBeNull()
  })
})

describe('defaultKeySize', () => {
  it('is 3072 for rsa, 256 otherwise', () => {
    expect(defaultKeySize('rsa')).toBe(3072)
    expect(defaultKeySize('RSA')).toBe(3072)
    expect(defaultKeySize('ed25519')).toBe(256)
    expect(defaultKeySize('ecdsa')).toBe(256)
  })
})

describe('normalizeCreatableKeyType', () => {
  it('lowercases and allows ed25519/rsa/ecdsa', () => {
    expect(normalizeCreatableKeyType('RSA')).toBe('rsa')
    expect(normalizeCreatableKeyType('ed25519')).toBe('ed25519')
    expect(normalizeCreatableKeyType('ECDSA')).toBe('ecdsa')
  })

  it('rejects unsupported types (e.g. dsa)', () => {
    expect(() => normalizeCreatableKeyType('dsa')).toThrow(/unsupported/i)
  })
})
