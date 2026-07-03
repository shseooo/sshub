// SSH command construction — ported from terminal.rs. Pure: given a server and
// already-resolved (existing) key/pem file paths, returns the `ssh` args. The
// caller resolves paths + checks existence and spawns the PTY.
//
// Auth options match the project rule exactly:
//   password → keyboard-interactive,password + PubkeyAuthentication=no
//   key/pem  → -i <path> + IdentitiesOnly=yes
//   agent    → PreferredAuthentications=publickey

import type { Server } from '@/types/server'

export interface SshPaths {
  /** Resolved private-key path for `key` auth, or null if missing. */
  keyPath?: string | null
  /** Resolved PEM path for `pem` auth, or null if missing. */
  pemPath?: string | null
}

export function buildSshArgs(server: Server, paths: SshPaths = {}): string[] {
  const args = [
    '-o', 'StrictHostKeyChecking=accept-new',
    '-o', 'ConnectTimeout=15',
    '-o', 'ServerAliveInterval=15',
    '-o', 'ServerAliveCountMax=3',
  ]

  if (server.port !== 22) args.push('-p', String(server.port))

  if (server.authType === 'password') {
    // Go straight to the password prompt — otherwise ssh sprays agent/default
    // keys and can hit MaxAuthTries before ever asking for a password.
    args.push('-o', 'PreferredAuthentications=keyboard-interactive,password')
    args.push('-o', 'PubkeyAuthentication=no')
  } else if (server.authType === 'pem') {
    if (paths.pemPath) args.push('-i', paths.pemPath, '-o', 'IdentitiesOnly=yes')
  } else if (server.authType === 'agent') {
    args.push('-o', 'PreferredAuthentications=publickey')
  } else {
    // 'key': use only the selected key (no agent spraying).
    if (paths.keyPath) args.push('-i', paths.keyPath, '-o', 'IdentitiesOnly=yes')
  }

  const pj = server.proxyJump?.trim()
  if (pj) args.push('-J', pj)

  args.push(`${server.username}@${server.host}`)
  return args
}

/** Connecting banner printed before ssh produces output. */
export function buildConnectBanner(server: Server): string {
  const pj = server.proxyJump?.trim()
  const jumpNote = pj ? ` -J ${pj}` : ''
  const portSuffix = server.port !== 22 ? `:${server.port}` : ''
  return (
    `\x1b[90m── sshub ──▶ ssh${jumpNote} ${server.username}@${server.host}${portSuffix} ` +
    `\x1b[0m(연결 중, 15초 내 응답 없으면 시간 초과)\r\n`
  )
}
