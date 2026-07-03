// Key/PEM file naming. Security boundary: names become file names, so anything
// outside [A-Za-z0-9_-] (notably `.` and `/`) is neutralized to `_` to block
// path traversal. Iterates by code point so a multi-unit char collapses to a
// single `_`.

export function keyFileName(name: string): string {
  const safe = Array.from(name, (c) => (/^[A-Za-z0-9_-]$/.test(c) ? c : '_')).join('')
  return `id_${safe}`
}

export function serverPemFileName(id: number): string {
  return `pem_server_${id}`
}
