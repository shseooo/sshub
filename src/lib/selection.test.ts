import { describe, it, expect } from 'vitest'
import { trimSelectionTrailing } from './selection'

describe('trimSelectionTrailing', () => {
  it('strips trailing spaces from each line', () => {
    expect(trimSelectionTrailing('name: foo     \ndesc: bar   ')).toBe('name: foo\ndesc: bar')
  })

  it('strips trailing tabs too', () => {
    expect(trimSelectionTrailing('a\t\t\nb')).toBe('a\nb')
  })

  it('keeps interior spaces (only the right padding is removed)', () => {
    expect(trimSelectionTrailing('| name: x        |   ')).toBe('| name: x        |')
  })

  it('preserves blank lines', () => {
    expect(trimSelectionTrailing('a   \n   \nb')).toBe('a\n\nb')
  })

  it('preserves a trailing CR (Windows line ends)', () => {
    expect(trimSelectionTrailing('a   \r\nb')).toBe('a\r\nb')
  })

  it('is a no-op when there is no trailing whitespace', () => {
    const s = 'clean\nlines'
    expect(trimSelectionTrailing(s)).toBe(s)
  })
})
