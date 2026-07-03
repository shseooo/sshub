// ~/.ssh/config parse + render. Pure; the I/O (read/write/backup) lives in the
// command layer.

import type { Server, CreateServerDto } from '@/types/server'

/** Parse an ssh config into server DTOs. Wildcard Host patterns are skipped. */
export function parseSshConfig(content: string): CreateServerDto[] {
  const entries: CreateServerDto[] = []
  let host: string | null = null
  let hostname: string | null = null
  let user: string | null = null
  let port = 22
  let proxyJump: string | null = null

  const flush = () => {
    if (host == null) return
    if (host.includes('*') || host.includes('?')) return // wildcard pattern
    entries.push({
      name: host,
      host: hostname ?? host,
      port,
      username: user ?? 'user',
      authType: 'key',
      proxyJump: proxyJump ?? undefined,
    })
  }

  for (const line of content.split('\n')) {
    const trimmed = line.trim()
    if (trimmed === '' || trimmed.startsWith('#')) continue

    if (trimmed.startsWith('Host ')) {
      flush()
      host = trimmed.slice('Host '.length).trim()
      hostname = null
      user = null
      port = 22
      proxyJump = null
      continue
    }

    // Split at the first '=' or whitespace.
    let i = -1
    for (let j = 0; j < trimmed.length; j++) {
      const c = trimmed[j]
      if (c === '=' || /\s/.test(c)) {
        i = j
        break
      }
    }
    if (i < 0) continue
    const key = trimmed.slice(0, i).trim().toLowerCase()
    const value = trimmed.slice(i + 1).trim()
    switch (key) {
      case 'hostname':
        hostname = value
        break
      case 'user':
        user = value
        break
      case 'port': {
        const n = parseInt(value, 10)
        port = Number.isNaN(n) ? 22 : n
        break
      }
      case 'proxyjump':
        proxyJump = value
        break
    }
  }
  flush()
  return entries
}

// ssh_config is line-oriented, so a newline (or other control char) embedded in
// a field value would let a crafted server name/host/user inject arbitrary
// directives — e.g. a `ProxyCommand` that runs on the next `ssh`. Strip control
// chars from every value before writing; legitimate names/hosts/users never
// contain them. Untrusted values reach here via config import round-trips and
// shared/edited server entries.
function sanitizeConfigValue(value: string): string {
  // Keep everything except C0 control chars (< 0x20, incl. CR/LF/TAB) and DEL
  // (0x7f). Printable Unicode (e.g. non-ASCII names) is preserved.
  let out = ''
  for (const ch of value) {
    const code = ch.charCodeAt(0)
    if (code >= 0x20 && code !== 0x7f) out += ch
  }
  return out
}

/** Render all servers to ssh config text (overwrites ~/.ssh/config). */
export function renderSshConfig(servers: Server[]): string {
  let out = ''
  for (const s of servers) {
    const name = sanitizeConfigValue(s.name)
    const group = s.groupName ? sanitizeConfigValue(s.groupName) : ''
    const displayName = group !== '' ? `${group}-${name}` : name
    out += `Host ${displayName}\n`
    out += `    HostName ${sanitizeConfigValue(s.host)}\n`
    out += `    Port ${s.port}\n`
    out += `    User ${sanitizeConfigValue(s.username)}\n`
    out += '\n'
  }
  return out
}
