// Per-session terminal scrollback persistence. The live PTY is not revived
// across restarts, but the serialized output history is, so a restored terminal
// shows what it printed before. Stored as one file per session id.

import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { scrollbackFileName } from './lib/scrollback'

export class ScrollbackStore {
  constructor(private readonly dir: string) {
    mkdirSync(dir, { recursive: true })
  }

  private pathFor(sessionId: string): string {
    return join(this.dir, scrollbackFileName(sessionId))
  }

  save(sessionId: string, data: string): void {
    writeFileSync(this.pathFor(sessionId), data, 'utf8')
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
