import { describe, expect, it } from 'vitest'
import { insertAt, leaves, removeLeaf, splitAt } from './TerminalContext'
import type { PaneNode, TerminalLeaf, TerminalSplit } from '@/types/terminal'

const leaf = (sessionId: string): TerminalLeaf => ({
  type: 'leaf',
  sessionId,
  serverId: null,
  label: sessionId,
})

const ids = (node: PaneNode) => leaves(node).map((l) => l.sessionId)

describe('leaves', () => {
  it('returns the node itself for a lone leaf', () => {
    expect(ids(leaf('a'))).toEqual(['a'])
  })

  it('flattens a nested split tree left-to-right', () => {
    const tree: TerminalSplit = {
      type: 'split',
      id: 's0',
      direction: 'row',
      sizes: [50, 50],
      children: [
        leaf('a'),
        { type: 'split', id: 's1', direction: 'column', sizes: [50, 50], children: [leaf('b'), leaf('c')] },
      ],
    }
    expect(ids(tree)).toEqual(['a', 'b', 'c'])
  })
})

describe('splitAt', () => {
  it('wraps a lone leaf in a split with the addition after it', () => {
    const out = splitAt(leaf('a'), 'a', 'row', leaf('b'))
    expect(out.type).toBe('split')
    expect(ids(out)).toEqual(['a', 'b'])
    expect((out as TerminalSplit).direction).toBe('row')
  })

  it('adds a sibling (flat) when the parent split has the same direction', () => {
    const row = splitAt(leaf('a'), 'a', 'row', leaf('b')) // row[a,b]
    const out = splitAt(row, 'b', 'row', leaf('c')) // still one row
    expect(out.type).toBe('split')
    expect((out as TerminalSplit).children).toHaveLength(3)
    expect(ids(out)).toEqual(['a', 'b', 'c'])
  })

  it('nests when splitting in the cross direction (split-right then split-down)', () => {
    // Regression: split right (row), then split down on the same pane must
    // produce a nested column — not flatten into the row.
    const row = splitAt(leaf('a'), 'a', 'row', leaf('b')) // row[a,b]
    const out = splitAt(row, 'b', 'column', leaf('c')) as TerminalSplit
    expect(out.direction).toBe('row')
    expect(out.children).toHaveLength(2)
    const second = out.children[1] as TerminalSplit
    expect(second.type).toBe('split')
    expect(second.direction).toBe('column')
    expect(ids(second)).toEqual(['b', 'c'])
  })
})

describe('removeLeaf', () => {
  it('returns null when the only leaf is removed', () => {
    expect(removeLeaf(leaf('a'), 'a')).toBeNull()
  })

  it('collapses a split with a single remaining child into that child', () => {
    const row = splitAt(leaf('a'), 'a', 'row', leaf('b'))
    const out = removeLeaf(row, 'a')
    expect(out).not.toBeNull()
    expect(out!.type).toBe('leaf')
    expect((out as TerminalLeaf).sessionId).toBe('b')
  })

  it('keeps siblings when one of three is removed', () => {
    let row = splitAt(leaf('a'), 'a', 'row', leaf('b'))
    row = splitAt(row, 'b', 'row', leaf('c')) // row[a,b,c]
    expect(ids(removeLeaf(row, 'b')!)).toEqual(['a', 'c'])
  })
})

describe('insertAt', () => {
  it('splits a leaf, placing the addition before when requested', () => {
    const out = insertAt(leaf('a'), 'a', leaf('b'), 'column', true)
    expect(ids(out)).toEqual(['b', 'a'])
    expect((out as TerminalSplit).direction).toBe('column')
  })

  it('grafts a whole subtree next to the target (cross-tab merge)', () => {
    const subtree = splitAt(leaf('x'), 'x', 'row', leaf('y')) // row[x,y]
    const out = insertAt(leaf('a'), 'a', subtree, 'column', false)
    expect((out as TerminalSplit).direction).toBe('column')
    expect(ids(out)).toEqual(['a', 'x', 'y'])
  })
})
