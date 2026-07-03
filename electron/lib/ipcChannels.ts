// Channels the renderer may subscribe to via window.electronAPI.on(). Only the
// per-session terminal streams are legitimate; anything else is rejected so a
// compromised renderer can't listen on arbitrary main→renderer IPC channels.

export const ALLOWED_LISTEN_PREFIXES = ['terminal-output-', 'terminal-closed-'] as const

export function isAllowedListenChannel(channel: string): boolean {
  return ALLOWED_LISTEN_PREFIXES.some((p) => channel.startsWith(p))
}
