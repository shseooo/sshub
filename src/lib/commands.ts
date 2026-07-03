import { invoke } from '@/lib/bridge'
import type { CreateServerDto, Server, UpdateServerDto } from '@/types/server'
import type { CreateKeyDto, ImportKeyDto, UpdateKeyDto, SshKey } from '@/types/key'

// ==================== Server Commands ====================

export function getServers(): Promise<Server[]> {
  return invoke<Server[]>('get_servers')
}

export function getServerById(id: number): Promise<Server | null> {
  return invoke<Server | null>('get_server', { id })
}

export function createServer(server: CreateServerDto): Promise<Server> {
  return invoke<Server>('create_server', { server })
}

export function updateServer(server: UpdateServerDto): Promise<Server> {
  return invoke<Server>('update_server', { server })
}

export function deleteServer(id: number): Promise<void> {
  return invoke<void>('delete_server', { id })
}

export function toggleFavorite(id: number): Promise<Server> {
  return invoke<Server>('toggle_favorite', { id })
}

// ==================== SSH Config Commands ====================

export function syncServersToConfig(): Promise<void> {
  return invoke<void>('sync_servers_to_config')
}

export function syncConfigToServers(): Promise<Server[]> {
  return invoke<Server[]>('sync_config_to_servers')
}

// ==================== SSH Key Commands ====================

export function getSshKeys(): Promise<SshKey[]> {
  return invoke<SshKey[]>('get_ssh_keys')
}

export function createSshKey(keyData: CreateKeyDto): Promise<SshKey> {
  return invoke<SshKey>('create_ssh_key', { keyData })
}

export function importSshKey(keyData: ImportKeyDto): Promise<SshKey> {
  return invoke<SshKey>('import_ssh_key', { keyData })
}

export function updateSshKey(keyData: UpdateKeyDto): Promise<SshKey> {
  return invoke<SshKey>('update_ssh_key', { keyData })
}

/** Re-encrypt a stored private key with a new passphrase (empty = remove). */
export function changeKeyPassphrase(
  id: number,
  currentPassphrase?: string,
  newPassphrase?: string
): Promise<void> {
  return invoke<void>('change_key_passphrase', { id, currentPassphrase, newPassphrase })
}

export function deleteKey(id: number): Promise<void> {
  return invoke<void>('delete_ssh_key', { id })
}

export interface LoadedKeyFile {
  fileName: string
  publicKey: string | null
  privateKey: string | null
}

/** Reads a key file from disk; private keys also pull in the sibling .pub. */
export function loadKeyFile(path: string): Promise<LoadedKeyFile> {
  return invoke<LoadedKeyFile>('load_key_file', { path })
}

/** Derive the public key from a private key (PEM) via `ssh-keygen -y`. */
export function derivePublicKeyFromPem(pem: string, passphrase?: string): Promise<string> {
  return invoke<string>('derive_public_key_from_pem', { pem, passphrase })
}

// ==================== Backup / Sync ====================

export interface ImportSummary {
  serversAdded: number
  serversSkipped: number
  keysAdded: number
  keysSkipped: number
  /** Shortcut prefs carried in the file, for the frontend to apply. */
  shortcuts?: Record<string, string> | null
}

export interface ExportOptions {
  passphrase?: string
  shortcuts?: Record<string, string>
  /** null/undefined = all servers; array = only these ids */
  serverIds?: number[] | null
  keyIds?: number[] | null
}

/** Exports selected (or all) servers/keys (+ optional shortcuts). With a passphrase, private keys are bundled and encrypted. */
export function exportData(path: string, opts: ExportOptions = {}): Promise<void> {
  return invoke<void>('export_data', {
    path,
    passphrase: opts.passphrase ?? null,
    shortcuts: opts.shortcuts ?? null,
    serverIds: opts.serverIds ?? null,
    keyIds: opts.keyIds ?? null,
  })
}

/** Merges an export file. Encrypted files require the passphrase (rejects with "ENCRYPTED" if missing). */
export function importData(path: string, passphrase?: string): Promise<ImportSummary> {
  return invoke<ImportSummary>('import_data', { path, passphrase: passphrase ?? null })
}

// ==================== Terminal Commands ====================

/** Spawns ssh (or a local shell when serverId is null) in a PTY. */
export function startTerminalSession(sessionId: string, serverId: number | null): Promise<void> {
  return invoke<void>('start_terminal_session', { sessionId, serverId })
}

export function writeTerminal(sessionId: string, data: string): Promise<void> {
  return invoke<void>('write_terminal', { sessionId, data })
}

export function resizeTerminal(sessionId: string, cols: number, rows: number): Promise<void> {
  return invoke<void>('resize_terminal', { sessionId, cols, rows })
}

export function closeTerminal(sessionId: string): Promise<void> {
  return invoke<void>('close_terminal', { sessionId })
}
