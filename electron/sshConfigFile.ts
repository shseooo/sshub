// ~/.ssh/config I/O — ported from ssh_config.rs. Pure parse/render live in
// ./lib/sshConfig; this handles the file read/write/backup and store merge.

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import type { Server } from '@/types/server'
import type { Store } from './store'
import { parseSshConfig, renderSshConfig } from './lib/sshConfig'

const MAX_CONFIG_BACKUPS = 10

function configPaths() {
  const home = homedir()
  return { dir: join(home, '.ssh'), path: join(home, '.ssh', 'config') }
}

/** Keep only the newest MAX_CONFIG_BACKUPS `config.bak.*` files (they accrue per sync). */
function pruneBackups(dir: string): void {
  try {
    const baks = readdirSync(dir)
      .filter((f) => f.startsWith('config.bak.'))
      .sort() // timestamp suffix sorts chronologically
    for (const f of baks.slice(0, Math.max(0, baks.length - MAX_CONFIG_BACKUPS))) {
      rmSync(join(dir, f), { force: true })
    }
  } catch {
    /* best-effort cleanup */
  }
}

/** Overwrite ~/.ssh/config from stored servers (backs up any existing file). */
export function syncServersToConfig(store: Store): void {
  const servers = store.listServers()
  if (servers.length === 0) {
    throw new Error('등록된 서버가 없어 ~/.ssh/config를 덮어쓰지 않았습니다.')
  }
  const { dir, path } = configPaths()
  mkdirSync(dir, { recursive: true })
  if (existsSync(path)) {
    const stamp = new Date().toISOString().replace(/[:.]/g, '-')
    copyFileSync(path, `${path}.bak.${stamp}`)
    pruneBackups(dir)
  }
  // Atomic write: render to a temp file then rename, so a crash mid-write can't
  // truncate ~/.ssh/config (external tools rely on it).
  const tmp = `${path}.tmp`
  writeFileSync(tmp, renderSshConfig(servers))
  renameSync(tmp, path)
}

/** Import hosts from ~/.ssh/config; skips names that already exist. */
export function syncConfigToServers(store: Store): Server[] {
  const { path } = configPaths()
  if (!existsSync(path)) return []
  const entries = parseSshConfig(readFileSync(path, 'utf8'))
  const existing = new Set(store.listServers().map((s) => s.name))
  const imported: Server[] = []
  for (const entry of entries) {
    if (existing.has(entry.name)) continue
    imported.push(store.insertServer(entry))
  }
  return imported
}
