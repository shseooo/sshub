// Key-management I/O — ported from key.rs. Private key material lives ONLY in
// 0600 files under ssh_keys/, never in the JSON store. ssh-keygen is shelled out
// for generation / passphrase change / public-key derivation, matching Tauri.

import { execFileSync } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { join, basename, extname } from 'node:path'
import type { Store } from './store'
import type { SshKey, CreateKeyDto, ImportKeyDto, UpdateKeyDto, KeyType } from '@/types/key'
import { keyFileName } from './lib/keyFiles'
import { detectKeyType, defaultKeySize, normalizeCreatableKeyType } from './lib/keyType'

export interface KeyCtx {
  store: Store
  keysDir: string
}

export interface LoadedKeyFile {
  fileName: string
  publicKey: string | null
  privateKey: string | null
}

function secureWrite(path: string, data: string): void {
  writeFileSync(path, data, { mode: 0o600 })
  chmodSync(path, 0o600)
}

/** Run ssh-keygen, returning stdout. Throws with stderr on failure. */
function keygen(args: string[]): string {
  try {
    return execFileSync('ssh-keygen', args, { encoding: 'utf8' })
  } catch (e) {
    const err = e as { stderr?: Buffer | string }
    const stderr = err.stderr ? String(err.stderr).trim() : String(e)
    throw new Error(stderr)
  }
}

/** Extract the public key from a private key (`ssh-keygen -y`). */
function derivePublicKey(keyPath: string, passphrase?: string): string {
  try {
    return keygen(['-y', '-f', keyPath, '-P', passphrase ?? '']).trim()
  } catch (e) {
    throw new Error(
      `개인 키에서 공개 키를 추출하지 못했습니다. 암호로 보호된 키라면 passphrase를 입력하세요. (${(e as Error).message})`
    )
  }
}

const keyPathFor = (ctx: KeyCtx, name: string) => join(ctx.keysDir, keyFileName(name))

export function getSshKeys(ctx: KeyCtx): SshKey[] {
  return ctx.store.listKeys().map((k) => ({
    ...k,
    hasPrivateFile: existsSync(keyPathFor(ctx, k.name)),
  }))
}

export function createSshKey(ctx: KeyCtx, dto: CreateKeyDto): SshKey {
  const keyType = normalizeCreatableKeyType(dto.keyType)
  const keySize = dto.keySize ?? defaultKeySize(keyType)
  const keyPath = keyPathFor(ctx, dto.name)
  if (existsSync(keyPath)) throw new Error(`Key file already exists: ${keyPath}`)
  const passphrase = dto.passphrase ?? ''

  const args = ['-t', keyType, '-f', keyPath, '-C', 'connectunnel-generated', '-N', passphrase]
  if (keyType === 'rsa') args.push('-b', String(keySize))
  keygen(args)

  const publicKey = readFileSync(`${keyPath}.pub`, 'utf8').trim()
  return ctx.store.insertKey({
    name: dto.name,
    publicKey,
    keyType: keyType as KeyType,
    keySize,
    passphraseProtected: passphrase !== '',
  })
}

export function importSshKey(ctx: KeyCtx, dto: ImportKeyDto): SshKey {
  let publicKey = (dto.publicKey ?? '').trim()

  if (dto.pemData != null) {
    const keyPath = keyPathFor(ctx, dto.name)
    secureWrite(keyPath, dto.pemData)
    if (!publicKey) {
      try {
        publicKey = derivePublicKey(keyPath, dto.passphrase)
      } catch {
        /* encrypted key without passphrase — leave empty */
      }
    }
  }

  if (!publicKey && dto.pemData == null) {
    throw new Error('공개 키 또는 개인 키(PEM) 중 하나는 필요합니다.')
  }

  const keyType = publicKey ? detectKeyType(publicKey) ?? dto.keyType : dto.keyType
  return ctx.store.insertKey({
    name: dto.name,
    publicKey,
    keyType,
    keySize: 256,
    passphraseProtected: !!(dto.passphrase && dto.passphrase.length > 0),
  })
}

export function updateSshKey(ctx: KeyCtx, dto: UpdateKeyDto): SshKey {
  const old = ctx.store.getKey(dto.id)
  const oldPriv = keyPathFor(ctx, old.name)
  const newPriv = keyPathFor(ctx, dto.name)

  // Rename the on-disk key (and its .pub) when the name changes.
  if (keyFileName(old.name) !== keyFileName(dto.name)) {
    if (existsSync(newPriv)) throw new Error('같은 이름의 키 파일이 이미 있습니다.')
    if (existsSync(oldPriv)) renameSync(oldPriv, newPriv)
    if (existsSync(`${oldPriv}.pub`)) renameSync(`${oldPriv}.pub`, `${newPriv}.pub`)
  }

  let passphraseProtected = old.passphraseProtected
  if (dto.pemData && dto.pemData.trim() !== '') {
    secureWrite(newPriv, dto.pemData)
    passphraseProtected = !!(dto.passphrase && dto.passphrase.length > 0)
  }

  const publicKey = (dto.publicKey ?? '').trim()
  const keyType = publicKey ? detectKeyType(publicKey) ?? dto.keyType : dto.keyType
  return ctx.store.updateKeyMeta({ id: dto.id, name: dto.name, publicKey, keyType, passphraseProtected })
}

export function changeKeyPassphrase(
  ctx: KeyCtx,
  id: number,
  currentPassphrase?: string,
  newPassphrase?: string
): void {
  const key = ctx.store.getKey(id)
  const path = keyPathFor(ctx, key.name)
  if (!existsSync(path)) throw new Error('이 기기에 개인 키 파일이 없습니다.')
  const next = newPassphrase ?? ''
  try {
    keygen(['-p', '-f', path, '-P', currentPassphrase ?? '', '-N', next])
  } catch (e) {
    throw new Error(`패스프레이즈 변경 실패 — 현재 패스프레이즈가 맞는지 확인하세요. (${(e as Error).message})`)
  }
  ctx.store.setKeyPassphraseProtected(id, next !== '')
}

export function deleteSshKey(ctx: KeyCtx, id: number): void {
  const key = ctx.store.findKey(id)
  if (key) {
    const priv = keyPathFor(ctx, key.name)
    rmSync(priv, { force: true })
    rmSync(`${priv}.pub`, { force: true })
  }
  ctx.store.deleteKey(id)
}

export function loadKeyFile(path: string): LoadedKeyFile {
  const content = readFileSync(path, 'utf8')
  const fileName = basename(path, extname(path))

  if (content.trimStart().startsWith('-----BEGIN')) {
    let publicKey: string | null = null
    if (existsSync(`${path}.pub`)) {
      publicKey = readFileSync(`${path}.pub`, 'utf8').trim()
    } else {
      try {
        const derived = derivePublicKey(path)
        if (derived) publicKey = derived
      } catch {
        /* encrypted bare key — derive at import with a passphrase */
      }
    }
    return { fileName, publicKey, privateKey: content }
  }
  return { fileName, publicKey: content.trim(), privateKey: null }
}

export function derivePublicKeyFromPem(ctx: KeyCtx, pem: string, passphrase?: string): string {
  if (pem.trim() === '') throw new Error('개인 키(PEM)가 비어 있습니다.')
  const tmp = join(ctx.keysDir, '.derive.tmp')
  secureWrite(tmp, pem)
  try {
    return derivePublicKey(tmp, passphrase)
  } finally {
    rmSync(tmp, { force: true })
  }
}
