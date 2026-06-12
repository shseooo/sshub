import { invoke } from '@tauri-apps/api/core'
import type { Server } from '@/types/server'
import type { SshKey } from '@/types/key'

// ==================== Server Commands ====================

export async function getServers(): Promise<Server[]> {
  return invoke<Server[]>('get_servers')
}

export async function getServerById(id: number): Promise<Server | null> {
  return invoke<Server | null>('get_server', { id })
}

export async function createServer(server: Partial<Server>): Promise<Server> {
  return invoke<Server>('create_server', { server })
}

export async function updateServer(server: Partial<Server>): Promise<Server> {
  return invoke<Server>('update_server', { server })
}

export async function deleteServer(id: number): Promise<void> {
  return invoke<void>('delete_server', { id })
}

export async function toggleFavorite(id: number): Promise<Server> {
  return invoke<Server>('toggle_favorite', { id })
}

// ==================== SSH Config Commands ====================

export async function syncServersToConfig(): Promise<void> {
  return invoke<void>('sync_servers_to_config')
}

export async function syncConfigToServers(): Promise<Server[]> {
  return invoke<Server[]>('sync_config_to_servers')
}

// ==================== SSH Key Commands ====================

export async function getSshKeys(): Promise<SshKey[]> {
  return invoke<SshKey[]>('get_ssh_keys')
}

export async function createSshKey(keyData: {
  name: string
  keyType: string
  keySize?: number
  passphrase?: string
}): Promise<SshKey> {
  return invoke<SshKey>('create_ssh_key', { keyData })
}

export async function importSshKey(keyData: {
  name: string
  publicKey: string
  privateKey?: string
  pemData?: string
  keyType: string
  passphrase?: string
}): Promise<SshKey> {
  return invoke<SshKey>('import_ssh_key', { keyData })
}

export async function deleteKey(id: number): Promise<void> {
  return invoke<void>('delete_ssh_key', { id })
}

// ==================== Terminal Commands ====================

export async function startSshSession(
  serverId: number,
  password?: string
): Promise<{ success: boolean; message: string; needsPassword: boolean }> {
  return invoke('start_ssh_session', { serverId, password })
}

export async function closeTerminalSession(sessionId: string): Promise<void> {
  return invoke<void>('close_ssh_session', { sessionId })
}
