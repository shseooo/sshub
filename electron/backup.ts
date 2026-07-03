// Backup export/import I/O. Plain exports are secret-free JSON; passphrase
// exports bundle the private key files and encrypt the whole thing
// (AES-256-GCM via ./lib/crypto).

import { chmodSync, existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { Store } from './store'
import type { ImportSummary } from './lib/bundleOps'
import { keyFileName } from './lib/keyFiles'
import { encryptBundle, decryptBundle, isEncryptedEnvelope } from './lib/crypto'

/** Returned to the frontend when an import file is encrypted but no passphrase
 *  was supplied — the UI then prompts and retries. */
const NEEDS_PASSPHRASE = 'ENCRYPTED'

export interface BackupCtx {
  store: Store
  keysDir: string
}

export interface ExportOptions {
  passphrase?: string | null
  shortcuts?: Record<string, string> | null
  serverIds?: number[] | null
  keyIds?: number[] | null
}

interface SecureBundle {
  bundle: ReturnType<Store['exportBundle']>
  privateKeys: { name: string; pem: string }[]
}

export function exportData(ctx: BackupCtx, path: string, opts: ExportOptions = {}): void {
  const bundle = ctx.store.exportBundle({
    serverIds: opts.serverIds ?? null,
    keyIds: opts.keyIds ?? null,
    shortcuts: opts.shortcuts ?? null,
  })

  if (opts.passphrase && opts.passphrase !== '') {
    const privateKeys: { name: string; pem: string }[] = []
    for (const key of bundle.keys) {
      const keyPath = join(ctx.keysDir, keyFileName(key.name))
      if (existsSync(keyPath)) privateKeys.push({ name: key.name, pem: readFileSync(keyPath, 'utf8') })
    }
    const secure: SecureBundle = { bundle, privateKeys }
    writeFileSync(path, encryptBundle(JSON.stringify(secure), opts.passphrase))
  } else {
    writeFileSync(path, JSON.stringify(bundle, null, 2))
  }
}

export function importData(ctx: BackupCtx, path: string, passphrase?: string | null): ImportSummary {
  const text = readFileSync(path, 'utf8')

  if (isEncryptedEnvelope(text)) {
    if (!passphrase || passphrase === '') throw new Error(NEEDS_PASSPHRASE)
    const secure = JSON.parse(decryptBundle(text, passphrase)) as SecureBundle
    const summary = ctx.store.importBundle(secure.bundle)
    // Restore private key files that don't already exist on this machine (0600).
    for (const entry of secure.privateKeys) {
      const keyPath = join(ctx.keysDir, keyFileName(entry.name))
      if (!existsSync(keyPath)) {
        writeFileSync(keyPath, entry.pem, { mode: 0o600 })
        chmodSync(keyPath, 0o600)
      }
    }
    return summary
  }

  return ctx.store.importBundle(JSON.parse(text))
}
