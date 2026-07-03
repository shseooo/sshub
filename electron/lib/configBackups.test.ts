import { describe, expect, it } from 'vitest'
import { backupsToPrune } from './configBackups'

const mk = (n: number) =>
  Array.from({ length: n }, (_, i) => `config.bak.2024-01-${String(i + 1).padStart(2, '0')}`)

describe('backupsToPrune', () => {
  it('keeps the newest `max` and returns the oldest for deletion', () => {
    const files = mk(13)
    const del = backupsToPrune(files, 10)
    expect(del).toHaveLength(3)
    expect(del).toEqual(files.slice(0, 3)) // oldest 3 (string-sorted ascending == chronological)
  })

  it('deletes nothing when at or under the cap', () => {
    expect(backupsToPrune(mk(10), 10)).toEqual([])
    expect(backupsToPrune(mk(4), 10)).toEqual([])
  })

  it('ignores files that are not config backups', () => {
    const files = ['config', 'known_hosts', 'config.bak.a', 'config.bak.b', 'id_rsa']
    expect(backupsToPrune(files, 1)).toEqual(['config.bak.a']) // only the oldest backup, never real files
  })

  it('is order-independent (sorts before slicing)', () => {
    const del = backupsToPrune(['config.bak.c', 'config.bak.a', 'config.bak.b'], 1)
    expect(del).toEqual(['config.bak.a', 'config.bak.b'])
  })
})
