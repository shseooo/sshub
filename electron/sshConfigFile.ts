// ~/.ssh/config I/O — ported from ssh_config.rs. Pure parse/render live in
// ./lib/sshConfig; this handles the file read/write/backup and store merge.

import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import type { Server } from '@/types/server'
import type { Store } from './store'
import { parseSshConfig, renderSshConfig } from './lib/sshConfig'

function configPaths() {
  const home = homedir()
  return { dir: join(home, '.ssh'), path: join(home, '.ssh', 'config') }
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
  }
  writeFileSync(path, renderSshConfig(servers))
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
