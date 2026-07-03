// Per-session last working directory for LOCAL terminals. The live PTY is not
// revived across restarts, so a restored local terminal would otherwise always
// reopen in the home directory. We snapshot each local session's cwd when the
// window closes (or the app quits) and respawn the shell there next launch.
// SSH sessions are excluded — the remote cwd is not ours to restore.

import { execFile } from 'node:child_process'
import { existsSync, readFileSync, readlinkSync, writeFileSync } from 'node:fs'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)

/**
 * Current working directory of a running process by pid, or null if it cannot be
 * determined (process gone, tool missing, unsupported platform). The pty's pid
 * is the login shell itself, whose cwd tracks the user's `cd`s.
 *
 * Async on purpose: this runs on the debounced scrollback-save beat while the
 * terminal is in active use. On macOS the cwd comes from spawning `lsof`, and a
 * synchronous spawn would block the main-process event loop (and thus every
 * terminal's I/O) for the tens-to-hundreds of ms lsof can take.
 */
export async function readPidCwd(pid: number): Promise<string | null> {
  try {
    if (process.platform === 'linux') {
      return readlinkSync(`/proc/${pid}/cwd`)
    }
    if (process.platform === 'darwin') {
      // `lsof -Fn -d cwd` prints the cwd path on a line prefixed with 'n'. Use the
      // absolute path: a GUI-launched (Finder/dock) app has a minimal PATH that may
      // not include /usr/sbin.
      const { stdout } = await execFileAsync(
        '/usr/sbin/lsof',
        ['-a', '-d', 'cwd', '-Fn', '-p', String(pid)],
        { encoding: 'utf8' }
      )
      const line = stdout.split('\n').find((l) => l.startsWith('n'))
      return line ? line.slice(1) : null
    }
  } catch {
    /* lsof missing, process already exited, or no /proc — fall back to null */
  }
  return null
}

/** JSON-backed map of sessionId → last known local cwd. */
export class TerminalCwdStore {
  private map: Record<string, string> = {}

  constructor(private readonly path: string) {}

  load(): void {
    try {
      const parsed = JSON.parse(readFileSync(this.path, 'utf8'))
      if (parsed && typeof parsed === 'object') this.map = parsed as Record<string, string>
    } catch {
      this.map = {}
    }
  }

  /** Saved cwd for a session, but only if it still exists on disk. */
  get(sessionId: string): string | null {
    const cwd = this.map[sessionId]
    return cwd && existsSync(cwd) ? cwd : null
  }

  set(sessionId: string, cwd: string): void {
    if (this.map[sessionId] === cwd) return
    this.map[sessionId] = cwd
    this.persist()
  }

  delete(sessionId: string): void {
    if (sessionId in this.map) {
      delete this.map[sessionId]
      this.persist()
    }
  }

  /** Drop entries for sessions no longer in the layout (mirrors scrollback prune). */
  prune(liveIds: string[]): void {
    const keep = new Set(liveIds)
    let changed = false
    for (const id of Object.keys(this.map)) {
      if (!keep.has(id)) {
        delete this.map[id]
        changed = true
      }
    }
    if (changed) this.persist()
  }

  private persist(): void {
    try {
      writeFileSync(this.path, JSON.stringify(this.map), { mode: 0o600 })
    } catch {
      /* best-effort: a failed cwd snapshot just means the next open uses home */
    }
  }
}
