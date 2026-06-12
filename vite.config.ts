import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [
    react(),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  // React 19 compatibility
  define: {
    global: 'globalThis',
  },
  // Preventes Vite from hogging the entire screen
  optimizeDeps: {
    exclude: ['xterm'],
  },
  server: {
    // Tauri expects the dev server to run on port 1420
    port: 1420,
    strictPort: true,
    watch: {
      // 3. Tauri will not pick up file changes on new top-level directories until `tauri dev` is re-started
      // unless you give a "fixed" list this is not a problem
      // https://github.com/tauri-apps/tauri/issues/5262#issuecomment-1363818759
      ignored: ['**/src-tauri/**'],
    },
  },
  envDir: './',
}))