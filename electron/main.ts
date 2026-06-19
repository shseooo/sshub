// Electron main process. Replaces the Tauri Rust backend. IPC commands mirror
// the Tauri command names so the React frontend (via src/lib/bridge.ts) is
// unchanged. Terminal uses node-pty; persistent data uses the JSON Store.
//
// Migration status: servers + store done. Keys/ssh_config/backup still stubbed.

import { app, BrowserWindow, dialog, ipcMain, type WebContents } from 'electron'
import { join } from 'node:path'
import { homedir } from 'node:os'
import { existsSync, mkdirSync, writeFileSync, chmodSync, rmSync } from 'node:fs'
import * as pty from 'node-pty'
import { Store } from './store'
import { buildSshArgs, buildConnectBanner } from './lib/ssh'
import { keyFileName, serverPemFileName } from './lib/keyFiles'
import * as keys from './keys'
import { syncServersToConfig, syncConfigToServers } from './sshConfigFile'
import * as backup from './backup'
import type { CreateServerDto, UpdateServerDto } from '@/types/server'
import type { CreateKeyDto, ImportKeyDto, UpdateKeyDto } from '@/types/key'

// Same path the Tauri build used (~/Library/Application Support/sshub.json on
// macOS) so existing servers/keys carry over. getPath('appData') is that base dir.
const appDataDir = app.getPath('appData')
const keysDir = join(appDataDir, 'ssh_keys')
const store = new Store(join(appDataDir, 'sshub.json'))
const keyCtx: keys.KeyCtx = { store, keysDir }

const sessions = new Map<string, pty.IPty>()

// Server PEMs (for `pem` auth) live in 0600 files keyed by server id — never in
// the JSON store. Mirrors the Tauri write_server_pem / remove_server_pem.
function writeServerPem(id: number, pem: string): void {
  const p = join(keysDir, serverPemFileName(id))
  writeFileSync(p, pem, { mode: 0o600 })
  chmodSync(p, 0o600)
}
function removeServerPem(id: number): void {
  rmSync(join(keysDir, serverPemFileName(id)), { force: true })
}

function createWindow() {
  const win = new BrowserWindow({
    width: 1000,
    height: 700,
    title: 'sshub',
    // Transparent backing so the macOS vibrancy material shows through wherever
    // the UI is translucent (the theme bakes alpha into --background). An opaque
    // backgroundColor would hide vibrancy entirely.
    backgroundColor: '#00000000',
    titleBarStyle: 'hiddenInset',
    vibrancy: 'hud', // macOS frosted glass (Chromium IME is unaffected by this)
    webPreferences: {
      preload: join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  })

  if (app.isPackaged) {
    // electron/out/main.cjs → ../../dist/index.html (dist sits at the app root).
    win.loadFile(join(__dirname, '..', '..', 'dist', 'index.html'))
  } else {
    win.loadURL(process.env.ELECTRON_RENDERER_URL || 'http://localhost:1420')
    win.webContents.openDevTools({ mode: 'detach' })
  }
}

/** Resolve the ssh command (or local shell) for a session. */
function resolveCommand(serverId: number | null): {
  file: string
  args: string[]
  banner: string | null
} {
  if (serverId == null) {
    return { file: process.env.SHELL || '/bin/zsh', args: ['-l'], banner: null }
  }
  const server = store.findServer(serverId)
  if (!server) return { file: process.env.SHELL || '/bin/zsh', args: ['-l'], banner: null }

  // Resolve existing key/pem files (the pure arg builder only adds -i when present).
  let keyPath: string | null = null
  let pemPath: string | null = null
  if (server.authType === 'pem') {
    const p = join(keysDir, serverPemFileName(server.id))
    if (existsSync(p)) pemPath = p
  } else if (server.authType === 'key' && server.keyId != null) {
    const key = store.findKey(server.keyId)
    if (key) {
      const p = join(keysDir, keyFileName(key.name))
      if (existsSync(p)) keyPath = p
    }
  }

  store.touchLastConnected(server.id)
  return { file: 'ssh', args: buildSshArgs(server, { keyPath, pemPath }), banner: buildConnectBanner(server) }
}

function startSession(sender: WebContents, sessionId: string, serverId: number | null) {
  const { file, args, banner } = resolveCommand(serverId)
  const p = pty.spawn(file, args, {
    name: 'xterm-256color',
    cols: 80,
    rows: 24,
    cwd: homedir(),
    env: process.env as Record<string, string>,
  })
  if (banner && !sender.isDestroyed()) sender.send(`terminal-output-${sessionId}`, banner)
  p.onData((d) => {
    if (!sender.isDestroyed()) sender.send(`terminal-output-${sessionId}`, d)
  })
  p.onExit(() => {
    if (!sender.isDestroyed()) sender.send(`terminal-closed-${sessionId}`, null)
    sessions.delete(sessionId)
  })
  sessions.set(sessionId, p)
}

ipcMain.handle('invoke', async (e, cmd: string, args: Record<string, unknown> = {}) => {
  switch (cmd) {
    // ---- servers ----
    case 'get_servers':
      return store.listServers()
    case 'get_server':
      return store.findServer(args.id as number)
    case 'create_server': {
      const dto = args.server as CreateServerDto
      const pem = dto.authType === 'pem' ? dto.pemData : undefined
      const created = store.insertServer(dto) // store strips pemData
      if (pem && pem.trim() !== '') writeServerPem(created.id, pem)
      return created
    }
    case 'update_server': {
      const dto = args.server as UpdateServerDto
      const updated = store.updateServer(dto)
      if (updated.authType !== 'pem') removeServerPem(updated.id)
      else if (dto.pemData && dto.pemData.trim() !== '') writeServerPem(updated.id, dto.pemData)
      return updated
    }
    case 'delete_server':
      removeServerPem(args.id as number)
      store.deleteServer(args.id as number)
      return null
    case 'toggle_favorite':
      return store.toggleFavorite(args.id as number)

    // ---- terminal ----
    case 'start_terminal_session':
      startSession(e.sender, args.sessionId as string, (args.serverId as number | null) ?? null)
      return null
    case 'write_terminal':
      sessions.get(args.sessionId as string)?.write(args.data as string)
      return null
    case 'resize_terminal':
      sessions.get(args.sessionId as string)?.resize(args.cols as number, args.rows as number)
      return null
    case 'close_terminal': {
      const p = sessions.get(args.sessionId as string)
      if (p) {
        p.kill()
        sessions.delete(args.sessionId as string)
      }
      return null
    }

    // ---- ssh keys ----
    case 'get_ssh_keys':
      return keys.getSshKeys(keyCtx)
    case 'create_ssh_key':
      return keys.createSshKey(keyCtx, args.keyData as CreateKeyDto)
    case 'import_ssh_key':
      return keys.importSshKey(keyCtx, args.keyData as ImportKeyDto)
    case 'update_ssh_key':
      return keys.updateSshKey(keyCtx, args.keyData as UpdateKeyDto)
    case 'change_key_passphrase':
      keys.changeKeyPassphrase(
        keyCtx,
        args.id as number,
        args.currentPassphrase as string | undefined,
        args.newPassphrase as string | undefined
      )
      return null
    case 'delete_ssh_key':
      keys.deleteSshKey(keyCtx, args.id as number)
      return null
    case 'load_key_file':
      return keys.loadKeyFile(args.path as string)
    case 'derive_public_key_from_pem':
      return keys.derivePublicKeyFromPem(keyCtx, args.pem as string, args.passphrase as string | undefined)

    // ---- native dialogs / paths ----
    case 'home_dir':
      return homedir()
    case 'dialog_open': {
      const o = args as { title?: string; defaultPath?: string; directory?: boolean; filters?: Electron.FileFilter[] }
      const r = await dialog.showOpenDialog({
        title: o.title,
        defaultPath: o.defaultPath,
        filters: o.filters,
        properties: [o.directory ? 'openDirectory' : 'openFile'],
      })
      return r.canceled || r.filePaths.length === 0 ? null : r.filePaths[0]
    }
    case 'dialog_save': {
      const o = args as { title?: string; defaultPath?: string; filters?: Electron.FileFilter[] }
      const r = await dialog.showSaveDialog({ title: o.title, defaultPath: o.defaultPath, filters: o.filters })
      return r.canceled || !r.filePath ? null : r.filePath
    }

    // ---- ssh_config sync ----
    case 'sync_servers_to_config':
      syncServersToConfig(store)
      return null
    case 'sync_config_to_servers':
      return syncConfigToServers(store)

    // ---- backup export / import ----
    case 'export_data':
      backup.exportData({ store, keysDir }, args.path as string, {
        passphrase: args.passphrase as string | null,
        shortcuts: args.shortcuts as Record<string, string> | null,
        serverIds: args.serverIds as number[] | null,
        keyIds: args.keyIds as number[] | null,
      })
      return null
    case 'import_data':
      return backup.importData({ store, keysDir }, args.path as string, args.passphrase as string | null)

    default:
      return null
  }
})

app.whenReady().then(() => {
  mkdirSync(keysDir, { recursive: true })
  store.load()
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  for (const p of sessions.values()) p.kill()
  sessions.clear()
  if (process.platform !== 'darwin') app.quit()
})
