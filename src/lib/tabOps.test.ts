import { describe, expect, it } from 'vitest'
import { insertAtIndex, reorderTabs, tabsExcept, tabsUpToInclusive } from './tabOps'

const tabs = (...ids: string[]) => ids.map((id) => ({ id }))
const ids = (arr: { id: string }[]) => arr.map((t) => t.id)

describe('reorderTabs', () => {
  it('moves a tab forward to an insertion boundary', () => {
    expect(ids(reorderTabs(tabs('a', 'b', 'c'), 'a', 2))).toEqual(['b', 'a', 'c'])
  })

  it('moves a tab to the end', () => {
    expect(ids(reorderTabs(tabs('a', 'b', 'c'), 'a', 3))).toEqual(['b', 'c', 'a'])
  })

  it('moves a tab backward', () => {
    expect(ids(reorderTabs(tabs('a', 'b', 'c'), 'c', 0))).toEqual(['c', 'a', 'b'])
  })

  it('is a no-op when dropped on its own boundary', () => {
    expect(ids(reorderTabs(tabs('a', 'b', 'c'), 'b', 1))).toEqual(['a', 'b', 'c'])
  })

  it('returns the array unchanged for an unknown id', () => {
    const input = tabs('a', 'b')
    expect(reorderTabs(input, 'zzz', 0)).toBe(input)
  })

  it('clamps an out-of-range index', () => {
    expect(ids(reorderTabs(tabs('a', 'b', 'c'), 'a', 99))).toEqual(['b', 'c', 'a'])
  })
})

describe('tabsExcept (close others)', () => {
  it('keeps only the given tab', () => {
    expect(ids(tabsExcept(tabs('a', 'b', 'c'), 'b'))).toEqual(['b'])
  })
})

describe('tabsUpToInclusive (close to right)', () => {
  it('keeps tabs up to and including the given one', () => {
    expect(ids(tabsUpToInclusive(tabs('a', 'b', 'c', 'd'), 'b'))).toEqual(['a', 'b'])
  })

  it('keeps everything when the target is last', () => {
    expect(ids(tabsUpToInclusive(tabs('a', 'b'), 'b'))).toEqual(['a', 'b'])
  })

  it('returns unchanged for an unknown id', () => {
    const input = tabs('a', 'b')
    expect(tabsUpToInclusive(input, 'zzz')).toBe(input)
  })
})

describe('insertAtIndex (detach placement)', () => {
  it('inserts at the requested boundary', () => {
    expect(insertAtIndex(['a', 'b', 'c'], 'x', 1)).toEqual(['a', 'x', 'b', 'c'])
  })

  it('appends when no index is given', () => {
    expect(insertAtIndex(['a', 'b'], 'x')).toEqual(['a', 'b', 'x'])
  })

  it('clamps a too-large index to the end', () => {
    expect(insertAtIndex(['a', 'b'], 'x', 99)).toEqual(['a', 'b', 'x'])
  })

  it('does not mutate the input', () => {
    const input = ['a', 'b']
    insertAtIndex(input, 'x', 0)
    expect(input).toEqual(['a', 'b'])
  })
})
