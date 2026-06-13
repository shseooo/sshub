/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vitejs.dev/config/
export default defineConfig({
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
    // Tauri expects the dev server to run on port 1420
    port: 1420,
    strictPort: true,
    watch: {
      // Tauri will not pick up file changes on new top-level directories until `tauri dev` is re-started
      // unless you give a "fixed" list this is not a problem
      // https://github.com/tauri-apps/tauri/issues/5262#issuecomment-1363818759
      ignored: ['**/src-tauri/**'],
    },
  },
  envDir: './',
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
  },
})
