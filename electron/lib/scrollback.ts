// Scrollback persistence helpers. Session ids become file names, so anything
// outside [A-Za-z0-9_-] (notably `.` and `/`) is neutralized to block traversal.

export function scrollbackFileName(sessionId: string): string {
  const safe = Array.from(sessionId, (c) => (/^[A-Za-z0-9_-]$/.test(c) ? c : '_')).join('')
  return `${safe}.txt`
}

/** Max scrollback lines kept per terminal (passed to xterm SerializeAddon). */
export const SCROLLBACK_LINES = 1000
