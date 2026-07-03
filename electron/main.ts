// Electron main process. Dispatches a single `invoke` IPC channel (from
// src/lib/bridge.ts) to backend commands. Terminal uses node-pty; persistent
// data uses the JSON Store.

import { app, BrowserWindow, dialog, ipcMain, Menu, session, shell, type WebContents } from 'electron'
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
import { ScrollbackStore } from './scrollbackStore'
import { TerminalCwdStore, readPidCwd } from './terminalCwd'
import { loadWindowBounds, saveWindowBounds } from './lib/windowState'
import type { CreateServerDto, UpdateServerDto } from '@/types/server'
import type { CreateKeyDto, ImportKeyDto, UpdateKeyDto } from '@/types/key'

// ~/Library/Application Support/sshub.json on macOS.
// getPath('appData') is that base dir.
const appDataDir = app.getPath('appData')
const keysDir = join(appDataDir, 'ssh_keys')
const store = new Store(join(appDataDir, 'sshub.json'))
const keyCtx: keys.KeyCtx = { store, keysDir }
const scrollback = new ScrollbackStore(join(appDataDir, 'sshub_scrollback'))
const cwdStore = new TerminalCwdStore(join(appDataDir, 'sshub_terminal_cwd.json'))
const windowBoundsPath = join(appDataDir, 'sshub_window.json')

const sessions = new Map<string, { pty: pty.IPty; serverId: number | null }>()

/** Snapshot each local session's cwd so the next launch reopens it there. */
async function captureCwds() {
  await Promise.all(
    [...sessions].map(async ([id, s]) => {
      if (s.serverId != null) return // SSH cwd is remote — not ours to restore
      const cwd = await readPidCwd(s.pty.pid)
      if (cwd) cwdStore.set(id, cwd)
    })
  )
}

/** Kill every live PTY and clear the session map. */
function killAllSessions() {
  for (const s of sessions.values()) s.pty.kill()
  sessions.clear()
}

// File-system commands (load_key_file, export_data, import_data) must only touch
// paths the USER explicitly picked in a native open/save dialog — never an
// arbitrary path the renderer supplies. A native dialog can't be auto-driven by
// the renderer, so this set only ever holds real user choices, which turns those
// commands from an arbitrary file read/write primitive into a scoped one.
const dialogPaths = new Set<string>()
function assertDialogPath(path: string): void {
  if (!dialogPaths.has(path)) {
    throw new Error('허용되지 않은 경로입니다. 파일 선택 창을 통해 다시 시도하세요.')
  }
}

// Server PEMs (for `pem` auth) live in 0600 files keyed by server id — never in
// the JSON store.
function writeServerPem(id: number, pem: string): void {
  const p = join(keysDir, serverPemFileName(id))
  writeFileSync(p, pem, { mode: 0o600 })
  chmodSync(p, 0o600)
}
function removeServerPem(id: number): void {
  rmSync(join(keysDir, serverPemFileName(id)), { force: true })
}

// Native app menu with NO View section: sshub is a desktop app, not a web page,
// so we drop the browser-like affordances the default Electron menu ships with —
// reload (Cmd+R), force-reload, toggle devtools (Opt+Cmd+I), and zoom. Removing
// the menu items removes their accelerators. Keep the standard app/edit/window
// menus so Quit, copy/paste, and window management still work.
function buildAppMenu() {
  const isMac = process.platform === 'darwin'
  const template: Electron.MenuItemConstructorOptions[] = [
    ...(isMac
      ? [{ role: 'appMenu' as const }]
      : [{ label: 'File', submenu: [{ role: 'quit' as const }] }]),
    { role: 'editMenu' },
    { role: 'windowMenu' },
  ]
  Menu.setApplicationMenu(Menu.buildFromTemplate(template))
}

function createWindow() {
  // Restore the size/position from the last session (centered by the OS on a
  // fresh install or if the saved geometry is unusable).
  const bounds = loadWindowBounds(windowBoundsPath, { width: 1000, height: 700 })
  const win = new BrowserWindow({
    ...bounds,
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
      sandbox: true,
      webSecurity: true,
    },
  })

  // The renderer never legitimately opens new windows or navigates away from the
  // app itself — external links go through the `open_external` IPC + shell. Deny
  // both so a renderer compromise can't pivot to arbitrary pages that would still
  // hold our IPC surface.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//i.test(url)) shell.openExternal(url)
    return { action: 'deny' }
  })
  win.webContents.on('will-navigate', (e, url) => {
    const devUrl = process.env.ELECTRON_RENDERER_URL || 'http://localhost:1420'
    if (!app.isPackaged && url.startsWith(devUrl)) return // allow HMR full reloads in dev
    e.preventDefault()
  })

  // A full reload (⌘R) or renderer crash re-mounts the whole tree, which respawns
  // every terminal under the same session ids. Kill the old PTYs so they don't
  // linger as orphans (SSH sessions especially keep the remote connection open).
  // SPA route changes are same-document navigations and are left untouched.
  win.webContents.on('did-start-navigation', (details) => {
    if (details.isMainFrame && !details.isSameDocument) killAllSessions()
  })
  win.webContents.on('render-process-gone', () => killAllSessions())

  // Persist geometry on resize/move (debounced) and once more on close, so the
  // next launch reopens where the user left it.
  let saveTimer: ReturnType<typeof setTimeout> | null = null
  const persist = () => {
    if (!win.isDestroyed()) saveWindowBounds(windowBoundsPath, win.getBounds())
  }
  const schedule = () => {
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = setTimeout(persist, 400)
  }
  win.on('resize', schedule)
  win.on('move', schedule)
  win.on('close', () => {
    if (saveTimer) clearTimeout(saveTimer)
    persist()
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

async function startSession(
  sender: WebContents,
  sessionId: string,
  serverId: number | null,
  cwdFromSessionId?: string
) {
  // Never leak a PTY by overwriting a live session with the same id — kill the
  // previous one first (e.g. a reload that respawns before cleanup ran).
  const existing = sessions.get(sessionId)
  if (existing) {
    existing.pty.kill()
    sessions.delete(sessionId)
  }
  const { file, args, banner } = resolveCommand(serverId)
  // Local shell cwd resolution:
  //  - a fresh local split inherits the focused pane's live cwd (cwdFromSessionId),
  //  - otherwise a restored session reopens in its last saved cwd,
  //  - falling back to home. SSH always starts at the remote home.
  let cwd = homedir()
  if (serverId == null) {
    const src = cwdFromSessionId ? sessions.get(cwdFromSessionId) : undefined
    if (src && src.serverId == null) {
      cwd = (await readPidCwd(src.pty.pid)) || homedir()
    } else if (!cwdFromSessionId) {
      cwd = cwdStore.get(sessionId) || homedir()
    }
  }
  const p = pty.spawn(file, args, {
    name: 'xterm-256color',
    cols: 80,
    rows: 24,
    cwd,
    env: process.env as Record<string, string>,
  })
  if (banner && !sender.isDestroyed()) sender.send(`terminal-output-${sessionId}`, banner)

  // Coalesce PTY output: node-pty emits many small chunks for bursty output
  // (build logs, `cat` of a big file), and one IPC message per chunk floods the
  // renderer. Buffer within a tick and flush once, cutting message count by
  // orders of magnitude while keeping latency imperceptible.
  let buf = ''
  let flushTimer: ReturnType<typeof setTimeout> | null = null
  const flush = () => {
    flushTimer = null
    if (buf === '' || sender.isDestroyed()) return
    const out = buf
    buf = ''
    sender.send(`terminal-output-${sessionId}`, out)
  }
  p.onData((d) => {
    buf += d
    if (!flushTimer) flushTimer = setTimeout(flush, 8)
  })
  p.onExit(() => {
    if (flushTimer) clearTimeout(flushTimer)
    flush() // deliver any buffered tail before the closed notice
    if (!sender.isDestroyed()) sender.send(`terminal-closed-${sessionId}`, null)
    sessions.delete(sessionId)
  })
  sessions.set(sessionId, { pty: p, serverId })
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
      await startSession(
        e.sender,
        args.sessionId as string,
        (args.serverId as number | null) ?? null,
        args.cwdFromSessionId as string | undefined
      )
      return null
    case 'write_terminal':
      sessions.get(args.sessionId as string)?.pty.write(args.data as string)
      return null
    case 'resize_terminal':
      sessions.get(args.sessionId as string)?.pty.resize(args.cols as number, args.rows as number)
      return null
    case 'close_terminal': {
      const s = sessions.get(args.sessionId as string)
      if (s) {
        s.pty.kill()
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
    case 'load_key_file': {
      const p = args.path as string
      assertDialogPath(p)
      return keys.loadKeyFile(p)
    }
    case 'derive_public_key_from_pem':
      return keys.derivePublicKeyFromPem(keyCtx, args.pem as string, args.passphrase as string | undefined)

    // ---- open a URL in the default browser (terminal link Cmd+click) ----
    case 'open_external': {
      const url = String(args.url ?? '')
      if (/^https?:\/\//i.test(url)) await shell.openExternal(url)
      return null
    }
    case 'reveal_path': {
      const p = String(args.path ?? '')
      const resolved = p.startsWith('~/') ? join(homedir(), p.slice(2)) : p
      if (existsSync(resolved)) shell.showItemInFolder(resolved)
      return null
    }

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
      if (r.canceled || r.filePaths.length === 0) return null
      dialogPaths.add(r.filePaths[0])
      return r.filePaths[0]
    }
    case 'dialog_save': {
      const o = args as { title?: string; defaultPath?: string; filters?: Electron.FileFilter[] }
      const r = await dialog.showSaveDialog({ title: o.title, defaultPath: o.defaultPath, filters: o.filters })
      if (r.canceled || !r.filePath) return null
      dialogPaths.add(r.filePath)
      return r.filePath
    }

    // ---- ssh_config sync ----
    case 'sync_servers_to_config':
      syncServersToConfig(store)
      return null
    case 'sync_config_to_servers':
      return syncConfigToServers(store)

    // ---- backup export / import ----
    case 'export_data': {
      const p = args.path as string
      assertDialogPath(p)
      backup.exportData({ store, keysDir }, p, {
        passphrase: args.passphrase as string | null,
        shortcuts: args.shortcuts as Record<string, string> | null,
        serverIds: args.serverIds as number[] | null,
        keyIds: args.keyIds as number[] | null,
      })
      return null
    }
    case 'import_data': {
      const p = args.path as string
      assertDialogPath(p)
      return backup.importData({ store, keysDir }, p, args.passphrase as string | null)
    }

    // ---- terminal scrollback persistence ----
    case 'scrollback_save': {
      const sid = args.sessionId as string
      scrollback.save(sid, args.data as string)
      // Snapshot the local session's cwd on the same (debounced) beat as scrollback
      // — i.e. shortly after each command settles — so the last directory survives
      // no matter how the session ends (shell exit, pane close, quit, or crash).
      const s = sessions.get(sid)
      if (s && s.serverId == null) {
        const cwd = await readPidCwd(s.pty.pid)
        if (cwd) cwdStore.set(sid, cwd)
      }
      return null
    }
    case 'scrollback_load':
      return scrollback.load(args.sessionId as string)
    case 'scrollback_delete':
      scrollback.delete(args.sessionId as string)
      cwdStore.delete(args.sessionId as string)
      return null
    case 'scrollback_prune':
      scrollback.prune(args.ids as string[])
      cwdStore.prune(args.ids as string[])
      return null

    default:
      return null
  }
})

/**
 * Content-Security-Policy for the renderer. We load only bundled assets plus the
 * Google Fonts stylesheet/webfonts, so lock everything else down; this is the
 * backstop that keeps a renderer/XSS compromise away from the IPC surface.
 * Dev additionally needs inline/eval scripts and the localhost HMR socket.
 */
function contentSecurityPolicy(): string {
  const fontCss = 'https://fonts.googleapis.com'
  const fontFiles = 'https://fonts.gstatic.com'
  if (app.isPackaged) {
    return [
      "default-src 'self'",
      "script-src 'self'",
      `style-src 'self' 'unsafe-inline' ${fontCss}`,
      `font-src 'self' data: ${fontFiles}`,
      "img-src 'self' data:",
      "connect-src 'self'",
      "object-src 'none'",
      "base-uri 'none'",
      "frame-src 'none'",
    ].join('; ')
  }
  return [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline' 'unsafe-eval'",
    `style-src 'self' 'unsafe-inline' ${fontCss}`,
    `font-src 'self' data: ${fontFiles}`,
    "img-src 'self' data:",
    `connect-src 'self' ws://localhost:* http://localhost:* ${fontCss} ${fontFiles}`,
    "object-src 'none'",
  ].join('; ')
}

app.whenReady().then(() => {
  session.defaultSession.webRequest.onHeadersReceived((details, cb) => {
    cb({
      responseHeaders: {
        ...details.responseHeaders,
        'Content-Security-Policy': [contentSecurityPolicy()],
      },
    })
  })
  buildAppMenu()
  mkdirSync(keysDir, { recursive: true })
  store.load()
  cwdStore.load()
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
}).catch((e) => {
  // Never fail silently with no window — tell the user what broke.
  dialog.showErrorBox('sshub 시작 실패', String(e instanceof Error ? e.stack || e.message : e))
})

// cwd is read from the LIVE shell (via lsof), so we must snapshot before killing
// — and because the read is async, quit must wait for it to finish.
async function snapshotAndReap() {
  await captureCwds() // snapshot local cwds before the shells die so reopen restores them
  killAllSessions()
}

app.on('window-all-closed', async () => {
  await snapshotAndReap()
  if (process.platform !== 'darwin') app.quit()
})

// Covers the quit paths window-all-closed doesn't (Cmd+Q with the window open,
// app.quit()). before-quit fires BEFORE windows close, so if we killed PTYs here
// the later cwd snapshot would read dead processes and lose the path. Instead,
// defer the quit until we've snapshotted the still-live shells, then quit for real.
let reaped = false
app.on('before-quit', (e) => {
  if (reaped) return
  e.preventDefault()
  snapshotAndReap().finally(() => {
    reaped = true
    app.quit()
  })
})

// A stray exception or rejection in a callback (pty.onData, fire-and-forget IPC)
// must not silently take down the main process and every session with it. Log
// and keep running.
process.on('uncaughtException', (err) => {
  console.error('uncaughtException in main process:', err)
})
process.on('unhandledRejection', (reason) => {
  console.error('unhandledRejection in main process:', reason)
})
