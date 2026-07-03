import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { SerializeAddon } from '@xterm/addon-serialize'
import { SearchAddon, type ISearchOptions } from '@xterm/addon-search'
import { WebglAddon } from '@xterm/addon-webgl'
import { listen, loadScrollback, saveScrollback, deleteScrollback, openExternal, revealPath } from '@/lib/bridge'
import { findFilePaths } from '@/lib/filePaths'
import { trimSelectionTrailing } from '@/lib/selection'
import { startTerminalSession, resizeTerminal, closeTerminal } from '@/lib/commands'
import type { Theme } from '@/lib/theme'

// Keep the last N lines of each terminal's output so a restored session shows
// its prior history (the live PTY is gone, but the scrollback returns).
const SCROLLBACK_LINES = 1000
const SCROLLBACK_DEBOUNCE_MS = 1500

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
  /** Swallow one stray non-composing input right after this terminal gains focus
   *  (Chromium re-inserts a just-committed IME syllable into the newly focused
   *  field on tab switch — see attachPhantomGuard). */
  ignorePhantom: boolean
  serialize: SerializeAddon
  search: SearchAddon
  saveTimer: ReturnType<typeof setTimeout> | null
  /** Whether the viewport is parked at the bottom. Startup restore + late reflows
   *  (webfont atlas rebuild, tab-show fit) must keep the terminal glued to the
   *  bottom, but never yank a user who has scrolled up. */
  pinBottom: boolean
  /** True while WE are fitting/scrolling the terminal, so the onScroll handler can
   *  ignore reflow-induced scroll events and only react to genuine user scrolls. */
  reflowing: boolean
  /** Deferred restore+session-start, run once the pane first has a real size.
   *  Writing scrollback into a hidden (0-size) terminal builds the buffer at the
   *  wrong dimensions and leaves a broken scroll/viewport when it's later shown,
   *  so background tabs stay empty until first displayed. Null once run. */
  hydrate: (() => void) | null
}

// Search highlight colors: all matches get a faint phosphor-green wash; the
// active (focused) match stands out in amber with a border.
const SEARCH_DECORATIONS = {
  matchBackground: '#3dff8833',
  matchOverviewRuler: '#3dff88',
  activeMatchBackground: '#ffae57',
  activeMatchBorder: '#ffd9a0',
  activeMatchColorOverviewRuler: '#ffae57',
}

const FONT = '"IBM Plex Mono", Menlo, Monaco, "Courier New", monospace'

// IBM Plex Mono arrives asynchronously (Google Fonts @import, display=swap). The
// WebGL/canvas renderer bakes glyphs into a texture atlas at open() time; if the
// web font has not loaded yet it bakes the fallback font's metrics and never
// notices the later font swap, so text stays garbled until a resize rebuilds the
// atlas. Resolve once the real font is ready so each terminal can rebuild itself.
const fontReady: Promise<void> = (async () => {
  try {
    await document.fonts.load('16px "IBM Plex Mono"')
    await document.fonts.ready
  } catch {
    /* Font Loading API unavailable → nothing to wait for */
  }
})()

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
  private focused: string | null = null
  cfg: PoolConfig

  constructor(cfg: PoolConfig) {
    this.cfg = cfg
    // macOS can corrupt the WebGL glyph atlas while the window is occluded or the
    // GPU sleeps — glyphs render as garbage tiles until something rebuilds the
    // atlas (historically: a manual window resize). The context is not reported
    // lost in this case, so rebake every atlas whenever the app returns to the
    // foreground; it's cheap and glyphs re-rasterize lazily.
    window.addEventListener('focus', this.rebakeAtlases)
    document.addEventListener('visibilitychange', this.onVisibilityChange)
  }

  private rebakeAtlases = () => {
    for (const e of this.entries.values()) {
      try {
        e.term.clearTextureAtlas()
        e.term.refresh(0, e.term.rows - 1)
      } catch {
        /* terminal mid-teardown */
      }
    }
  }

  private onVisibilityChange = () => {
    if (document.visibilityState === 'visible') this.rebakeAtlases()
  }

  private create(sessionId: string, serverId: number | null): Entry {
    const container = document.createElement('div')
    container.className = 'h-full w-full'
    const term = new XTerm({
      cursorBlink: true,
      fontSize: this.cfg.theme.termFontSize,
      fontFamily: FONT,
      theme: xtermTheme(this.cfg.theme),
      allowProposedApi: true, // search decorations use xterm's proposed decoration API
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    // Cmd/Ctrl+click a URL opens it in the default browser (a plain click does
    // nothing, to avoid accidental opens from terminal output).
    term.loadAddon(
      new WebLinksAddon((event, uri) => {
        if (event.metaKey || event.ctrlKey) openExternal(uri).catch(() => {})
      })
    )
    const serialize = new SerializeAddon()
    term.loadAddon(serialize)
    const search = new SearchAddon()
    term.loadAddon(search)
    term.open(container)

    const d1 = term.onData((data) => {
      // Typing jumps back to the newest output (standard terminal behavior) and
      // re-pins, so subsequent output/reflows keep following the bottom.
      const e = this.entries.get(sessionId)
      if (e) {
        e.pinBottom = true
        this.stickBottom(e)
      }
      this.cfg.onInput(sessionId, data)
    })
    const d2 = term.onResize(({ cols, rows }) => {
      resizeTerminal(sessionId, cols, rows).catch(() => {})
    })
    const ro = new ResizeObserver(() => {
      const e = this.entries.get(sessionId)
      if (e) this.fitEntry(e)
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
      ignorePhantom: false,
      serialize,
      search,
      saveTimer: null,
      pinBottom: true,
      reflowing: false,
      hydrate: null,
    }
    this.entries.set(sessionId, entry)

    // Track the user's scroll intent: pin to bottom while they're at the bottom,
    // release when they scroll up. Ignore scrolls we cause ourselves (fit reflow,
    // scrollToBottom) — those must never flip the pin off, or a reflow that lands
    // a line short of the bottom would permanently un-stick the terminal.
    entry.disps.push(
      term.onScroll(() => {
        if (entry.reflowing) return
        const buf = term.buffer.active
        entry.pinBottom = buf.viewportY >= buf.baseY
      })
    )

    // GPU renderer for fast large-output scrolling. A GPU reset (sleep/wake,
    // driver restart) drops the WebGL context; instead of permanently falling
    // back to the slow DOM renderer, try to re-acquire a fresh context a few
    // times. Each attempt rebuilds the glyph atlas from scratch, so a reset
    // also recovers any atlas corruption.
    let webglRetries = 0
    const loadWebgl = () => {
      try {
        const webgl = new WebglAddon()
        webgl.onContextLoss(() => {
          webgl.dispose()
          if (!entry.disposed && webglRetries++ < 3) setTimeout(loadWebgl, 1000)
        })
        term.loadAddon(webgl)
      } catch {
        /* WebGL unavailable → xterm keeps its DOM renderer */
      }
    }
    loadWebgl()

    // When the web font finally lands, drop the atlas that was baked with the
    // fallback font and remeasure, so the first render after the swap is crisp
    // without the user having to resize the window. Resolves immediately for
    // every terminal created after the font is already cached.
    fontReady.then(() => {
      if (entry.disposed) return
      try {
        term.clearTextureAtlas()
        this.fitEntry(entry) // fit + re-anchor; font-swap reflow must not drift off bottom
        term.refresh(0, term.rows - 1)
      } catch {
        /* terminal torn down between scheduling and running this callback */
      }
    })

    // On tab switch, Chromium re-inserts a just-committed IME syllable into the
    // newly focused terminal as a stray non-composing `input`. Swallow that one
    // event (capture phase, before xterm's own input handler) so the syllable
    // stays only in the tab it was composed in.
    const phantomGuard = (ev: Event) => {
      if (ev.target !== term.textarea || !entry.ignorePhantom) return
      entry.ignorePhantom = false
      if ((ev as InputEvent).isComposing) return
      ev.stopImmediatePropagation()
      if (term.textarea) term.textarea.value = ''
    }
    container.addEventListener('input', phantomGuard, true)
    entry.disps.push({ dispose: () => container.removeEventListener('input', phantomGuard, true) })

    // xterm fills its cell-based selection out to the rightmost column, so copying
    // box-drawing/TUI output drags trailing spaces onto every line. Match VS Code:
    // intercept copy (capture phase, before xterm's own copy handler) and write the
    // trimmed selection instead. Only act when trimming actually changes something.
    const trimCopy = (ev: ClipboardEvent) => {
      if (!term.hasSelection()) return
      const sel = term.getSelection()
      const trimmed = trimSelectionTrailing(sel)
      if (trimmed === sel) return
      ev.clipboardData?.setData('text/plain', trimmed)
      ev.preventDefault()
      ev.stopImmediatePropagation()
    }
    container.addEventListener('copy', trimCopy, true)
    entry.disps.push({ dispose: () => container.removeEventListener('copy', trimCopy, true) })

    // Local sessions only: make absolute file paths Cmd/Ctrl+clickable → reveal in
    // Finder. Skipped for SSH sessions (those paths are remote, not local files).
    if (serverId == null) {
      entry.disps.push(
        term.registerLinkProvider({
          provideLinks(y, callback) {
            const line = term.buffer.active.getLine(y - 1)?.translateToString(true) ?? ''
            const matches = findFilePaths(line)
            if (!matches.length) return callback(undefined)
            callback(
              matches.map((mt) => ({
                text: mt.text,
                range: { start: { x: mt.start + 1, y }, end: { x: mt.end, y } },
                activate: (event: MouseEvent) => {
                  if (event.metaKey || event.ctrlKey) revealPath(mt.text).catch(() => {})
                },
              }))
            )
          },
        })
      )
    }

    // Debounced persist of the rendered scrollback (capped to N lines).
    const scheduleSave = () => {
      if (entry.saveTimer) clearTimeout(entry.saveTimer)
      entry.saveTimer = setTimeout(() => {
        entry.saveTimer = null
        saveScrollback(sessionId, serialize.serialize({ scrollback: SCROLLBACK_LINES })).catch(() => {})
      }, SCROLLBACK_DEBOUNCE_MS)
    }

    // Restore scrollback + start the shell — but only once the pane is actually
    // shown at a real size (see Entry.hydrate). Writing into a 0-size hidden
    // terminal corrupts the wrap/scroll state, which is why background tabs looked
    // scrolled-up (and wheel-stuck) when first opened. fitEntry() triggers this.
    entry.hydrate = () => {
      entry.hydrate = null
      ;(async () => {
        const unOut = await listen<string>(`terminal-output-${sessionId}`, (e) => {
          // Follow the bottom AFTER the chunk is parsed (write callback), not
          // before — scrolling before parse lands on the old buffer end. Gated on
          // pinBottom so a user reading history isn't yanked down.
          term.write(e.payload, () => this.stickBottom(entry))
          scheduleSave()
        })
        const unClosed = await listen(`terminal-closed-${sessionId}`, () =>
          term.write(`\r\n\x1b[90m[${this.cfg.closedNotice}]\x1b[0m\r\n`)
        )
        if (entry.disposed) {
          unOut()
          unClosed()
          return
        }
        entry.unlisten.push(unOut, unClosed)

        // Show this session's prior output (read-only; the live shell below is new).
        try {
          const hist = await loadScrollback(sessionId)
          if (hist && !entry.disposed) {
            term.write(hist)
            // scrollToBottom must run once the restored buffer is actually parsed,
            // so anchor it in the separator write's callback (writes are async).
            term.write('\r\n\x1b[90m── 이전 기록 (세션은 새로 시작됨) ──\x1b[0m\r\n', () => {
              entry.pinBottom = true
              this.stickBottom(entry)
            })
          }
        } catch {
          /* no prior scrollback */
        }

        try {
          await startTerminalSession(sessionId, serverId)
          await resizeTerminal(sessionId, term.cols, term.rows)
        } catch (err) {
          term.write(`\r\n\x1b[31m${this.cfg.connectFail}: ${err}\x1b[0m\r\n`)
        }

        // xterm doesn't draw a terminal's cursor until it has been focused once,
        // so an unfocused split pane on restore showed no (hollow) cursor until
        // the user clicked into it. Give every freshly-hydrated pane one focus/blur
        // cycle to initialize its cursor render, then restore focus to whoever had
        // it (the focus effect focuses the active pane; siblings stay hollow).
        if (!entry.disposed && !term.textarea?.matches(':focus')) {
          const prev = this.focused
          term.focus()
          term.blur()
          if (prev && prev !== sessionId) this.entries.get(prev)?.term.focus()
        }
      })()
    }

    return entry
  }

  /** Scroll to the bottom if the viewport is pinned there, suppressing the
   *  onScroll reaction so our own scroll never flips the pin off. */
  private stickBottom(e: Entry) {
    if (!e.pinBottom || e.disposed) return
    e.reflowing = true
    try {
      e.term.scrollToBottom()
    } finally {
      e.reflowing = false
    }
  }

  /** Ensure a session exists and reparent its container into `parent`. */
  mountInto(sessionId: string, serverId: number | null, parent: HTMLElement) {
    let e = this.entries.get(sessionId)
    if (!e) e = this.create(sessionId, serverId)
    if (e.container.parentElement !== parent) parent.appendChild(e.container)
    this.refit(sessionId)
  }

  /** Fit an entry to its container and keep the viewport pinned to the bottom
   *  unless the user has scrolled up. Every fit path (this.fit, ResizeObserver,
   *  webfont-swap) must go through here, or a reflow drifts the scroll off bottom. */
  private fitEntry(e: Entry) {
    if (e.container.clientWidth <= 0 || e.container.clientHeight <= 0) return
    const stick = e.pinBottom
    e.reflowing = true // suppress onScroll reacting to the reflow/scroll we're about to cause
    try {
      e.fit.fit()
      if (stick) e.term.scrollToBottom()
    } finally {
      e.reflowing = false
    }
    // First time this pane has a real size: restore scrollback + start the shell
    // now, into a correctly-sized terminal (never while hidden).
    if (e.hydrate) e.hydrate()
  }

  fit(sessionId: string) {
    const e = this.entries.get(sessionId)
    if (e) this.fitEntry(e)
  }

  /** Fit now and again next frame — a freshly reparented/shown container may not
   *  have its final size until layout settles (detach/merge/collapse, tab show). */
  refit(sessionId: string) {
    this.fit(sessionId)
    requestAnimationFrame(() => this.fit(sessionId))
  }

  focus(sessionId: string) {
    const e = this.entries.get(sessionId)
    if (!e) return
    // Commit any in-flight IME composition on the previously focused terminal
    // before moving focus, so its marked text (e.g. a half-composed Hangul
    // syllable) commits to its own PTY instead of being carried into the new one.
    if (this.focused && this.focused !== sessionId) {
      this.entries.get(this.focused)?.term.textarea?.blur()
      // Arm the phantom-input guard: Chromium may push a just-committed IME
      // syllable into this freshly focused terminal. Disarm next tick so it
      // never swallows real typing.
      e.ignorePhantom = true
      setTimeout(() => {
        e.ignorePhantom = false
      }, 0)
    }
    this.focused = sessionId
    e.term.focus()
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
    if (e.saveTimer) clearTimeout(e.saveTimer)
    e.ro.disconnect()
    e.disps.forEach((d) => d.dispose())
    e.unlisten.forEach((u) => u())
    closeTerminal(sessionId).catch(() => {})
    e.term.dispose()
    e.container.remove()
    this.entries.delete(sessionId)
    // The pane was explicitly closed/reconnected (not an app quit), so its
    // saved history is no longer wanted.
    deleteScrollback(sessionId).catch(() => {})
  }

  /** Dispose every session not in `live` (closed panes/tabs, reconnect swaps). */
  disposeExcept(live: Set<string>) {
    for (const [id, e] of this.entries) if (!live.has(id)) this.disposeEntry(id, e)
  }

  // ==================== Search ====================

  private searchOpts(opts?: { caseSensitive?: boolean }): ISearchOptions {
    return { decorations: SEARCH_DECORATIONS, caseSensitive: opts?.caseSensitive ?? false }
  }

  searchNext(sessionId: string, query: string, opts?: { caseSensitive?: boolean }): boolean {
    return this.entries.get(sessionId)?.search.findNext(query, this.searchOpts(opts)) ?? false
  }

  searchPrevious(sessionId: string, query: string, opts?: { caseSensitive?: boolean }): boolean {
    return this.entries.get(sessionId)?.search.findPrevious(query, this.searchOpts(opts)) ?? false
  }

  clearSearch(sessionId: string) {
    this.entries.get(sessionId)?.search.clearDecorations()
  }

  /** Clear search highlights on every terminal (the active pane can change
   *  mid-search in a split tab, so closing must clear all, not just one). */
  clearAllSearch() {
    for (const e of this.entries.values()) e.search.clearDecorations()
  }

  /** Persist every live terminal's scrollback now (best-effort, on app quit). */
  flushScrollback() {
    for (const [id, e] of this.entries) {
      if (e.saveTimer) {
        clearTimeout(e.saveTimer)
        e.saveTimer = null
      }
      // Never-hydrated tabs have an empty buffer; serializing it would clobber
      // their still-valid saved history with nothing.
      if (e.hydrate) continue
      saveScrollback(id, e.serialize.serialize({ scrollback: SCROLLBACK_LINES })).catch(() => {})
    }
  }

  disposeAll() {
    window.removeEventListener('focus', this.rebakeAtlases)
    document.removeEventListener('visibilitychange', this.onVisibilityChange)
    for (const [id, e] of this.entries) this.disposeEntry(id, e)
  }
}
