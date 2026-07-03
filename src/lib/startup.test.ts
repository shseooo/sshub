import { afterEach, describe, expect, it } from 'vitest'
import { loadStartRoute, saveStartRoute, START_ROUTES } from './startup'

describe('start route persistence', () => {
  afterEach(() => localStorage.clear())

  it('defaults to / when nothing is stored', () => {
    expect(loadStartRoute()).toBe('/')
  })

  it('round-trips a valid route', () => {
    saveStartRoute('/keys')
    expect(loadStartRoute()).toBe('/keys')
  })

  it('falls back to / for an unknown or stale stored route', () => {
    localStorage.setItem('start-route', '/does-not-exist')
    expect(loadStartRoute()).toBe('/')
  })

  it('accepts every advertised START_ROUTES entry', () => {
    for (const { route } of START_ROUTES) {
      saveStartRoute(route)
      expect(loadStartRoute()).toBe(route)
    }
  })
})
