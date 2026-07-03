// Passphrase encryption for backup exports — AES-256-GCM with a scrypt-derived
// key. Envelope is JSON so it's easy to detect on import.

import { randomBytes, scryptSync, createCipheriv, createDecipheriv } from 'node:crypto'

const MAGIC = 'sshub-enc-v1'

interface Envelope {
  magic: string
  salt: string
  iv: string
  ct: string
  tag: string
}

export function encryptBundle(plaintext: string, passphrase: string): string {
  const salt = randomBytes(16)
  const iv = randomBytes(12)
  const key = scryptSync(passphrase, salt, 32)
  const cipher = createCipheriv('aes-256-gcm', key, iv)
  const ct = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()])
  const env: Envelope = {
    magic: MAGIC,
    salt: salt.toString('base64'),
    iv: iv.toString('base64'),
    ct: ct.toString('base64'),
    tag: cipher.getAuthTag().toString('base64'),
  }
  return JSON.stringify(env)
}

export function decryptBundle(envelope: string, passphrase: string): string {
  const env = JSON.parse(envelope) as Envelope
  if (env.magic !== MAGIC) throw new Error('암호화된 sshub 백업 파일이 아닙니다.')
  const key = scryptSync(passphrase, Buffer.from(env.salt, 'base64'), 32)
  const decipher = createDecipheriv('aes-256-gcm', key, Buffer.from(env.iv, 'base64'))
  decipher.setAuthTag(Buffer.from(env.tag, 'base64'))
  try {
    return Buffer.concat([decipher.update(Buffer.from(env.ct, 'base64')), decipher.final()]).toString('utf8')
  } catch {
    throw new Error('복호화 실패: 암호가 틀렸거나 파일이 손상되었습니다.')
  }
}

export function isEncryptedEnvelope(text: string): boolean {
  try {
    return (JSON.parse(text) as Envelope).magic === MAGIC
  } catch {
    return false
  }
}
