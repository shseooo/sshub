// IPC bridge to the Electron main process (window.electronAPI, from
// electron/preload.cjs). All renderer→backend calls go through here so call
// sites stay decoupled from the Electron API surface.
//
// (Earlier this also routed to Tauri/WKWebView; Tauri was dropped in v0.2.0
// because WKWebView delivers CJK/IME input without composition events, which
// xterm.js can't handle. Chromium fires proper composition events.)

interface ElectronAPI {
  invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown>
  on(channel: string, cb: (payload: unknown) => void): () => void
}

const electron = (globalThis as unknown as { electronAPI?: ElectronAPI }).electronAPI

function api(): ElectronAPI {
  if (!electron) throw new Error('electronAPI unavailable (run inside Electron, not a plain browser)')
  return electron
}

export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return api().invoke(cmd, args) as Promise<T>
}

export function listen<T>(event: string, handler: (e: { payload: T }) => void): Promise<() => void> {
  const off = api().on(event, (payload) => handler({ payload: payload as T }))
  return Promise.resolve(off)
}

export interface DialogFilter {
  name: string
  extensions: string[]
}

export interface OpenFileOpts {
  title?: string
  defaultPath?: string
  directory?: boolean
  filters?: DialogFilter[]
}

/** Native open-file dialog → selected path (or null if cancelled). */
export function openFileDialog(opts: OpenFileOpts = {}): Promise<string | null> {
  return api().invoke('dialog_open', { ...opts }) as Promise<string | null>
}

export interface SaveFileOpts {
  title?: string
  defaultPath?: string
  filters?: DialogFilter[]
}

/** Native save-file dialog → chosen path (or null if cancelled). */
export function saveFileDialog(opts: SaveFileOpts = {}): Promise<string | null> {
  return api().invoke('dialog_save', { ...opts }) as Promise<string | null>
}

export function homeDir(): Promise<string> {
  return api().invoke('home_dir') as Promise<string>
}

/** Open an http(s) URL in the OS default browser. */
export function openExternal(url: string): Promise<void> {
  return api().invoke('open_external', { url }) as Promise<void>
}

// ---- terminal scrollback (output history, restored across restarts) ----

export function saveScrollback(sessionId: string, data: string): Promise<void> {
  return api().invoke('scrollback_save', { sessionId, data }) as Promise<void>
}

export function loadScrollback(sessionId: string): Promise<string | null> {
  return api().invoke('scrollback_load', { sessionId }) as Promise<string | null>
}

export function deleteScrollback(sessionId: string): Promise<void> {
  return api().invoke('scrollback_delete', { sessionId }) as Promise<void>
}

export function pruneScrollback(ids: string[]): Promise<void> {
  return api().invoke('scrollback_prune', { ids }) as Promise<void>
}
