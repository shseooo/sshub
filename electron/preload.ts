// Electron preload — exposes a minimal, shell-agnostic IPC surface to the
// renderer. src/lib/bridge.ts routes invoke()/listen() here when present.

import { contextBridge, ipcRenderer } from 'electron'
import { isAllowedListenChannel } from './lib/ipcChannels'

contextBridge.exposeInMainWorld('electronAPI', {
  invoke: (cmd: string, args?: Record<string, unknown>) => ipcRenderer.invoke('invoke', cmd, args),
  on: (channel: string, cb: (payload: unknown) => void) => {
    if (!isAllowedListenChannel(channel)) {
      return () => {} // reject unknown channels; nothing to unsubscribe
    }
    const listener = (_e: unknown, payload: unknown) => cb(payload)
    ipcRenderer.on(channel, listener)
    return () => ipcRenderer.removeListener(channel, listener)
  },
})
