import { describe, it, expect } from 'vitest'
import { keyFileName, serverPemFileName } from './keyFiles'

describe('keyFileName (security boundary — sanitize)', () => {
  it('replaces unsafe chars with underscore and prefixes id_', () => {
    expect(keyFileName('my key!')).toBe('id_my_key_')
    expect(keyFileName('ok-name_1')).toBe('id_ok-name_1')
  })

  it('neutralizes path-traversal characters (. and /)', () => {
    expect(keyFileName('../etc/passwd')).toBe('id____etc_passwd')
    expect(keyFileName('a/../b')).toBe('id_a____b')
  })

  it('keeps only ASCII alphanumerics, dash, underscore', () => {
    expect(keyFileName('aZ09-_')).toBe('id_aZ09-_')
    expect(keyFileName('é한')).toBe('id___') // non-ASCII → one underscore each
  })

  it('handles empty name', () => {
    expect(keyFileName('')).toBe('id_')
  })
})

describe('serverPemFileName', () => {
  it('builds pem_server_<id>', () => {
    expect(serverPemFileName(7)).toBe('pem_server_7')
  })
})
