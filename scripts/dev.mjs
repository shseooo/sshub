// One-command dev launcher: start vite, wait for it, then run Electron.
// `bun run app`. No extra deps. Closing the Electron window (or Ctrl-C) stops both.

import { spawn } from 'node:child_process'
import http from 'node:http'

const VITE_URL = 'http://localhost:1420/'
const procs = []

function run(cmd, args) {
  const p = spawn(cmd, args, { stdio: 'inherit' })
  procs.push(p)
  return p
}

let killing = false
function killAll() {
  if (killing) return
  killing = true
  for (const p of procs) {
    try {
      p.kill()
    } catch {
      /* already gone */
    }
  }
}

process.on('SIGINT', () => {
  killAll()
  process.exit(0)
})
process.on('exit', killAll)

// 1) vite dev server
run('bun', ['run', 'dev'])

// 2) poll until vite answers, then launch Electron
function waitForVite(retries = 60) {
  http
    .get(VITE_URL, (res) => {
      res.destroy()
      startElectron()
    })
    .on('error', () => {
      if (retries <= 0) {
        console.error('[dev] vite did not come up on :1420')
        killAll()
        process.exit(1)
      }
      setTimeout(() => waitForVite(retries - 1), 500)
    })
}

function startElectron() {
  const e = run('bun', ['run', 'electron'])
  // When the Electron app quits, tear down vite and exit.
  e.on('exit', (code) => {
    killAll()
    process.exit(code ?? 0)
  })
}

waitForVite()
