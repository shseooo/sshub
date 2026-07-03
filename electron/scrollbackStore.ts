// Per-session terminal scrollback persistence. The live PTY is not revived
// across restarts, but the serialized output history is, so a restored terminal
// shows what it printed before. Stored as one file per session id.

import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { scrollbackFileName } from './lib/scrollback'

export class ScrollbackStore {
  constructor(private readonly dir: string) {
    // 0700: scrollback captures on-screen output, which can contain secrets a
    // user printed (tokens, `cat id_rsa`, env dumps). Keep it owner-only.
    mkdirSync(dir, { recursive: true, mode: 0o700 })
    try {
      chmodSync(dir, 0o700) // tighten a dir created before this (mkdir mode only applies on create)
    } catch {
      /* best-effort */
    }
  }

  private pathFor(sessionId: string): string {
    return join(this.dir, scrollbackFileName(sessionId))
  }

  save(sessionId: string, data: string): void {
    // 0600 for the same reason as the dir — never world-readable.
    writeFileSync(this.pathFor(sessionId), data, { encoding: 'utf8', mode: 0o600 })
  }

  load(sessionId: string): string | null {
    const p = this.pathFor(sessionId)
    return existsSync(p) ? readFileSync(p, 'utf8') : null
  }

  delete(sessionId: string): void {
    rmSync(this.pathFor(sessionId), { force: true })
  }

  /** Drop scrollback files for sessions no longer in the layout (orphans). */
  prune(liveIds: string[]): void {
    const keep = new Set(liveIds.map(scrollbackFileName))
    for (const f of readdirSync(this.dir)) {
      if (f.endsWith('.txt') && !keep.has(f)) rmSync(join(this.dir, f), { force: true })
    }
  }
}
