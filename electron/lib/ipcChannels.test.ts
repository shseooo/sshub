import { describe, expect, it } from 'vitest'
import { isAllowedListenChannel } from './ipcChannels'

describe('isAllowedListenChannel (renderer IPC-subscribe whitelist)', () => {
  it('allows per-session terminal output/closed channels', () => {
    expect(isAllowedListenChannel('terminal-output-abc123')).toBe(true)
    expect(isAllowedListenChannel('terminal-closed-abc123')).toBe(true)
  })

  it('rejects arbitrary or look-alike channels', () => {
    for (const c of [
      '',
      'invoke',
      'store-changed',
      'terminal-', // bare prefix, not a real channel
      'terminal-input-1', // not an exposed channel
      'x-terminal-output-1', // does not start with an allowed prefix
      '__proto__',
    ]) {
      expect(isAllowedListenChannel(c)).toBe(false)
    }
  })
})
