// Detect absolute file paths in a terminal line so they can be made clickable
// (Cmd+click → reveal in Finder). Heuristic + local-session only; existence is
// verified at click time. Only absolute (`/` or `~/`) paths — relative paths
// need a cwd we don't track, and remote-session paths aren't local files.

export interface FilePathMatch {
  text: string
  /** 0-based char offset of the path start in the line. */
  start: number
  /** 0-based char offset one past the path end. */
  end: number
}

// Path must start at line-start or after a delimiter (whitespace or one of
// ([{=<'"). `:` is intentionally excluded so the "//host" of a URL like
// https://… isn't matched, and a slash inside a word (a/b) is skipped too.
const PATH_RE = /(^|[\s([{=<'"])((?:~\/|\/)[A-Za-z0-9._+\-@/]+)/g
const TRAILING = /[.,;:)\]}>'"]+$/

export function findFilePaths(line: string): FilePathMatch[] {
  const out: FilePathMatch[] = []
  for (const m of line.matchAll(PATH_RE)) {
    const lead = m[1].length
    const start = (m.index ?? 0) + lead
    let text = m[2]
    const trimmed = text.replace(TRAILING, '')
    text = trimmed
    if (text.length < 2 || !text.includes('/')) continue
    out.push({ text, start, end: start + text.length })
  }
  return out
}
