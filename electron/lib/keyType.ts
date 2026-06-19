// Key-type helpers — ported from key.rs (detect_key_type + create defaults).

import type { KeyType } from '@/types/key'

/** Map an OpenSSH public-key prefix to our stored key-type label. */
export function detectKeyType(publicKey: string): KeyType | null {
  const prefix = publicKey.trim().split(/\s+/)[0]
  if (!prefix) return null
  if (prefix === 'ssh-ed25519' || prefix === 'sk-ssh-ed25519@openssh.com') return 'ed25519'
  if (prefix === 'ssh-rsa') return 'rsa'
  if (prefix === 'ssh-dss') return 'dsa'
  if (prefix.startsWith('ecdsa-') || prefix.startsWith('sk-ecdsa-')) return 'ecdsa'
  return null
}

export function defaultKeySize(keyType: string): number {
  return keyType.toLowerCase() === 'rsa' ? 3072 : 256
}

const CREATABLE = ['ed25519', 'rsa', 'ecdsa']

/** Lowercase + validate a key type allowed for ssh-keygen generation. */
export function normalizeCreatableKeyType(keyType: string): string {
  const t = keyType.toLowerCase()
  if (!CREATABLE.includes(t)) throw new Error(`Unsupported key type: ${t}`)
  return t
}
