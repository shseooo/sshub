import { describe, it, expect } from 'vitest'
import { buildSshArgs, buildConnectBanner } from './ssh'
import type { Server } from '@/types/server'

function srv(over: Partial<Server> = {}): Server {
  return {
    id: 1, name: 's', host: 'example.com', port: 22, username: 'root',
    authType: 'key', keyId: null, pemData: null, proxyJump: null, groupName: null,
    tags: null, isFavorite: false, notes: null, lastConnectedAt: null,
    createdAt: null, updatedAt: null, ...over,
  }
}

const BASE = ['-o', 'StrictHostKeyChecking=accept-new', '-o', 'ConnectTimeout=15',
  '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3']

describe('buildSshArgs', () => {
  it('starts with the standard -o options and ends with user@host', () => {
    const a = buildSshArgs(srv())
    expect(a.slice(0, BASE.length)).toEqual(BASE)
    expect(a[a.length - 1]).toBe('root@example.com')
  })

  it('adds -p only for non-default ports', () => {
    expect(buildSshArgs(srv({ port: 22 }))).not.toContain('-p')
    const a = buildSshArgs(srv({ port: 2222 }))
    expect(a).toContain('-p')
    expect(a[a.indexOf('-p') + 1]).toBe('2222')
  })

  it('password auth: keyboard-interactive/password + PubkeyAuthentication=no, no -i', () => {
    const a = buildSshArgs(srv({ authType: 'password' }))
    expect(a).toContain('PreferredAuthentications=keyboard-interactive,password')
    expect(a).toContain('PubkeyAuthentication=no')
    expect(a).not.toContain('-i')
  })

  it('key auth with a resolved key path: -i <path> + IdentitiesOnly=yes', () => {
    const a = buildSshArgs(srv({ authType: 'key', keyId: 3 }), { keyPath: '/keys/id_mykey' })
    expect(a).toContain('-i')
    expect(a[a.indexOf('-i') + 1]).toBe('/keys/id_mykey')
    expect(a).toContain('IdentitiesOnly=yes')
  })

  it('key auth with no resolved path (file missing): no -i', () => {
    expect(buildSshArgs(srv({ authType: 'key', keyId: 3 }))).not.toContain('-i')
  })

  it('pem auth with a resolved pem path: -i <pem> + IdentitiesOnly=yes', () => {
    const a = buildSshArgs(srv({ authType: 'pem' }), { pemPath: '/keys/pem_server_1' })
    expect(a[a.indexOf('-i') + 1]).toBe('/keys/pem_server_1')
    expect(a).toContain('IdentitiesOnly=yes')
  })

  it('agent auth: PreferredAuthentications=publickey, no -i', () => {
    const a = buildSshArgs(srv({ authType: 'agent' }))
    expect(a).toContain('PreferredAuthentications=publickey')
    expect(a).not.toContain('-i')
  })

  it('proxyJump: adds -J <trimmed>, ignores blank', () => {
    const a = buildSshArgs(srv({ proxyJump: '  user@bastion  ' }))
    expect(a).toContain('-J')
    expect(a[a.indexOf('-J') + 1]).toBe('user@bastion')
    expect(buildSshArgs(srv({ proxyJump: '   ' }))).not.toContain('-J')
  })

  it('orders -J before the destination', () => {
    const a = buildSshArgs(srv({ proxyJump: 'b' }))
    expect(a.indexOf('-J')).toBeLessThan(a.indexOf('root@example.com'))
  })
})

describe('buildConnectBanner', () => {
  it('includes destination and jump host', () => {
    const b = buildConnectBanner(srv({ proxyJump: 'jh', port: 2222 }))
    expect(b).toContain('root@example.com')
    expect(b).toContain('-J jh')
    expect(b).toContain(':2222')
  })

  it('omits port suffix for default port', () => {
    expect(buildConnectBanner(srv({ port: 22 }))).not.toContain(':22')
  })
})
