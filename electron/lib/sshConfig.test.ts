import { describe, it, expect } from 'vitest'
import { parseSshConfig, renderSshConfig } from './sshConfig'
import type { Server } from '@/types/server'

describe('parseSshConfig (ported from ssh_config.rs)', () => {
  it('parses a basic host block', () => {
    const e = parseSshConfig('Host web\n  HostName 10.0.0.1\n  User deploy\n  Port 2222\n')
    expect(e).toHaveLength(1)
    expect(e[0]).toMatchObject({ name: 'web', host: '10.0.0.1', username: 'deploy', port: 2222, authType: 'key' })
  })

  it('maps ProxyJump', () => {
    const e = parseSshConfig('Host internal\n  HostName 10.0.0.9\n  ProxyJump jump@bastion\n')
    expect(e[0].proxyJump).toBe('jump@bastion')
  })

  it('skips wildcard patterns', () => {
    const e = parseSshConfig('Host *\n  User nobody\n\nHost real\n  HostName example.com\n')
    expect(e.map((x) => x.name)).toEqual(['real'])
  })

  it('applies defaults for missing fields', () => {
    const e = parseSshConfig('Host bare\n')
    expect(e[0]).toMatchObject({ name: 'bare', host: 'bare', port: 22, username: 'user', authType: 'key' })
  })

  it('supports key=value syntax', () => {
    const e = parseSshConfig('Host eq\n  HostName=1.2.3.4\n  Port=2200\n')
    expect(e[0]).toMatchObject({ host: '1.2.3.4', port: 2200 })
  })

  it('ignores comments and blank lines', () => {
    const e = parseSshConfig('# a comment\n\nHost x\n  HostName h\n')
    expect(e).toHaveLength(1)
    expect(e[0].name).toBe('x')
  })

  it('falls back to port 22 on an invalid port', () => {
    expect(parseSshConfig('Host x\n  Port nope\n')[0].port).toBe(22)
  })
})

function srv(over: Partial<Server> = {}): Server {
  return {
    id: 1, name: 'web', host: '10.0.0.1', port: 2222, username: 'deploy', authType: 'key',
    keyId: null, pemData: null, proxyJump: null, groupName: null, tags: null,
    isFavorite: false, notes: null, lastConnectedAt: null, createdAt: null, updatedAt: null, ...over,
  }
}

describe('renderSshConfig', () => {
  it('writes a Host block with HostName/Port/User', () => {
    const out = renderSshConfig([srv()])
    expect(out).toContain('Host web')
    expect(out).toContain('    HostName 10.0.0.1')
    expect(out).toContain('    Port 2222')
    expect(out).toContain('    User deploy')
  })

  it('prefixes the display name with the group when set', () => {
    expect(renderSshConfig([srv({ groupName: 'prod' })])).toContain('Host prod-web')
    expect(renderSshConfig([srv({ groupName: '' })])).toContain('Host web')
  })

  it('round-trips through the parser', () => {
    const parsed = parseSshConfig(renderSshConfig([srv({ name: 'r', host: 'h', port: 2200, username: 'u' })]))
    expect(parsed[0]).toMatchObject({ name: 'r', host: 'h', port: 2200, username: 'u' })
  })
})
