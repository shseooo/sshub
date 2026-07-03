// Electron preload — exposes a minimal, shell-agnostic IPC surface to the
// renderer. src/lib/bridge.ts routes invoke()/listen() here when present.

import { contextBridge, ipcRenderer } from 'electron'

// The renderer only ever listens to per-session terminal channels. Whitelist
// those prefixes so a compromised renderer can't subscribe to arbitrary
// main→renderer IPC channels.
const ALLOWED_CHANNEL_PREFIXES = ['terminal-output-', 'terminal-closed-']

contextBridge.exposeInMainWorld('electronAPI', {
  invoke: (cmd: string, args?: Record<string, unknown>) => ipcRenderer.invoke('invoke', cmd, args),
  on: (channel: string, cb: (payload: unknown) => void) => {
    if (!ALLOWED_CHANNEL_PREFIXES.some((p) => channel.startsWith(p))) {
      return () => {} // reject unknown channels; nothing to unsubscribe
    }
    const listener = (_e: unknown, payload: unknown) => cb(payload)
    ipcRenderer.on(channel, listener)
    return () => ipcRenderer.removeListener(channel, listener)
  },
})
