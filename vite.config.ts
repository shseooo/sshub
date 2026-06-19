/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'
import { readFileSync } from 'node:fs'

const appVersion = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version

// https://vitejs.dev/config/
export default defineConfig({
  // Relative asset paths so the built index.html works under Electron's file://.
  base: './',
  // Inject the package.json version so the UI never drifts from the real version.
  define: { __APP_VERSION__: JSON.stringify(appVersion) },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  optimizeDeps: {
    exclude: ['@xterm/xterm'],
  },
  server: {
    // Electron's main process loads http://localhost:1420 in dev (see electron/main.ts).
    port: 1420,
    strictPort: true,
  },
  envDir: './',
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,tsx}', 'electron/**/*.{test,spec}.ts'],
  },
})
