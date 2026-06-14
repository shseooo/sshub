// Which menu the app opens on launch. Stored in localStorage like other UI prefs.

export const START_ROUTES = [
  { route: '/', labelKey: 'nav.dashboard' },
  { route: '/servers', labelKey: 'nav.servers' },
  { route: '/terminal', labelKey: 'nav.terminal' },
  { route: '/keys', labelKey: 'nav.keys' },
  { route: '/settings', labelKey: 'nav.settings' },
] as const

const KEY = 'start-route'
const VALID = new Set(START_ROUTES.map((r) => r.route as string))

export function loadStartRoute(): string {
  const v = localStorage.getItem(KEY)
  return v && VALID.has(v) ? v : '/'
}

export function saveStartRoute(route: string): void {
  localStorage.setItem(KEY, route)
}
