import { describe, it, expect } from 'vitest'
import { scrollbackFileName } from './scrollback'

describe('scrollbackFileName (security boundary)', () => {
  it('keeps a normal UUID session id and adds .txt', () => {
    expect(scrollbackFileName('3f2504e0-4f89-41d3-9a0c-0305e82c3301')).toBe(
      '3f2504e0-4f89-41d3-9a0c-0305e82c3301.txt'
    )
  })

  it('neutralizes path-traversal / unsafe characters', () => {
    expect(scrollbackFileName('../../etc/passwd')).toBe('______etc_passwd.txt')
    expect(scrollbackFileName('a/b')).toBe('a_b.txt')
    expect(scrollbackFileName('x.y')).toBe('x_y.txt')
  })

  it('handles empty id', () => {
    expect(scrollbackFileName('')).toBe('.txt')
  })
})
