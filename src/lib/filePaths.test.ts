import { describe, it, expect } from 'vitest'
import { findFilePaths } from './filePaths'

const texts = (line: string) => findFilePaths(line).map((m) => m.text)

describe('findFilePaths', () => {
  it('matches absolute paths', () => {
    expect(texts('see /Users/me/app.log for details')).toEqual(['/Users/me/app.log'])
  })

  it('matches ~ home paths', () => {
    expect(texts('open ~/projects/x/main.ts now')).toEqual(['~/projects/x/main.ts'])
  })

  it('strips a trailing :line:col and punctuation', () => {
    expect(texts('error at /var/log/app.log:42:3')).toEqual(['/var/log/app.log'])
    expect(texts('(/etc/hosts).')).toEqual(['/etc/hosts'])
  })

  it('ignores relative paths and bare slashes in words', () => {
    expect(texts('./build/out and a/b/c')).toEqual([])
  })

  it('does not match URLs', () => {
    expect(texts('visit https://example.com/path here')).toEqual([])
  })

  it('reports correct start/end offsets', () => {
    const m = findFilePaths('x /a/b y')[0]
    expect(m).toMatchObject({ text: '/a/b', start: 2, end: 6 })
  })

  it('finds multiple paths on a line', () => {
    expect(texts('/a/one /b/two')).toEqual(['/a/one', '/b/two'])
  })
})
