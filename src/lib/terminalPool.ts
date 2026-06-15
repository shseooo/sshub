import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { listen } from '@tauri-apps/api/event'
import { startTerminalSession, resizeTerminal, closeTerminal } from '@/lib/tauriCommands'
import type { Theme } from '@/lib/theme'

// Live config the host mutates each render so the pool always routes input,
// theme, and i18n strings with current values.
export interface PoolConfig {
  onInput: (sessionId: string, data: string) => void
  theme: Theme
  closedNotice: string
  connectFail: string
}

interface Entry {
  serverId: number | null
  container: HTMLDivElement
  term: XTerm
  fit: FitAddon
  ro: ResizeObserver
  disps: { dispose(): void }[]
  unlisten: (() => void)[]
  disposed: boolean
}

const FONT = '"IBM Plex Mono", Menlo, Monaco, "Courier New", monospace'

function xtermTheme(t: Theme) {
  return {
    background: t.termBg,
    foreground: t.termFg,
    cursor: t.accent,
    cursorAccent: t.termBg,
    selectionBackground: 'rgba(120, 120, 120, 0.35)',
  }
}

/**
 * Owns one xterm instance per session, mounted in a detached container that is
 * reparented into whichever pane currently shows it. Because the DOM node is
 * moved (not recreated), the terminal — buffer, scrollback, live PTY — survives
 * being dragged between tabs (merge/detach). Sessions are disposed only when
 * they truly disappear from the tab tree (`disposeExcept`).
 */
export class TerminalPool {
  private entries = new Map<string, Entry>()
  cfg: PoolConfig

  constructor(cfg: PoolConfig) {
    this.cfg = cfg
  }

  private create(sessionId: string, serverId: number | null): Entry {
    const container = document.createElement('div')
    container.className = 'h-full w-full'
    const term = new XTerm({
      cursorBlink: true,
      fontSize: this.cfg.theme.termFontSize,
      fontFamily: FONT,
      theme: xtermTheme(this.cfg.theme),
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.loadAddon(new WebLinksAddon())
    term.open(container)

    const d1 = term.onData((data) => this.cfg.onInput(sessionId, data))
    const d2 = term.onResize(({ cols, rows }) => {
      resizeTerminal(sessionId, cols, rows).catch(() => {})
    })
    const ro = new ResizeObserver(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) fit.fit()
    })
    ro.observe(container)

    const entry: Entry = {
      serverId,
      container,
      term,
      fit,
      ro,
      disps: [d1, d2],
      unlisten: [],
      disposed: false,
    }
    this.entries.set(sessionId, entry)

    ;(async () => {
      const unOut = await listen<string>(`terminal-output-${sessionId}`, (e) => term.write(e.payload))
      const unClosed = await listen(`terminal-closed-${sessionId}`, () =>
        term.write(`\r\n\x1b[90m[${this.cfg.closedNotice}]\x1b[0m\r\n`)
      )
      if (entry.disposed) {
        unOut()
        unClosed()
        return
      }
      entry.unlisten.push(unOut, unClosed)
      try {
        await startTerminalSession(sessionId, serverId)
        await resizeTerminal(sessionId, term.cols, term.rows)
      } catch (err) {
        term.write(`\r\n\x1b[31m${this.cfg.connectFail}: ${err}\x1b[0m\r\n`)
      }
    })()

    return entry
  }

  /** Ensure a session exists and reparent its container into `parent`. */
  mountInto(sessionId: string, serverId: number | null, parent: HTMLElement) {
    let e = this.entries.get(sessionId)
    if (!e) e = this.create(sessionId, serverId)
    if (e.container.parentElement !== parent) parent.appendChild(e.container)
    this.refit(sessionId)
  }

  fit(sessionId: string) {
    const e = this.entries.get(sessionId)
    if (e && e.container.clientWidth > 0 && e.container.clientHeight > 0) e.fit.fit()
  }

  /** Fit now and again next frame — a freshly reparented/shown container may not
   *  have its final size until layout settles (detach/merge/collapse, tab show). */
  refit(sessionId: string) {
    this.fit(sessionId)
    requestAnimationFrame(() => this.fit(sessionId))
  }

  focus(sessionId: string) {
    this.entries.get(sessionId)?.term.focus()
  }

  setTheme(theme: Theme) {
    this.cfg.theme = theme
    const opt = xtermTheme(theme)
    for (const [id, e] of this.entries) {
      e.term.options.theme = opt
      if (e.term.options.fontSize !== theme.termFontSize) {
        e.term.options.fontSize = theme.termFontSize
        this.refit(id) // font size changes the cell grid → recompute cols/rows
      }
    }
  }

  private disposeEntry(sessionId: string, e: Entry) {
    e.disposed = true
    e.ro.disconnect()
    e.disps.forEach((d) => d.dispose())
    e.unlisten.forEach((u) => u())
    closeTerminal(sessionId).catch(() => {})
    e.term.dispose()
    e.container.remove()
    this.entries.delete(sessionId)
  }

  /** Dispose every session not in `live` (closed panes/tabs, reconnect swaps). */
  disposeExcept(live: Set<string>) {
    for (const [id, e] of this.entries) if (!live.has(id)) this.disposeEntry(id, e)
  }

  disposeAll() {
    for (const [id, e] of this.entries) this.disposeEntry(id, e)
  }
}
