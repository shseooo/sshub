import { useEffect } from 'react'
import { Routes, Route, Navigate, useNavigate } from 'react-router-dom'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { loadStartRoute } from './lib/startup'
import Sidebar from './components/Sidebar'
import TerminalHost from './components/TerminalHost'
import { TerminalProvider } from './contexts/TerminalContext'
import Dashboard from './pages/Dashboard'
import ServerList from './pages/ServerList'
import ServerEdit from './pages/ServerEdit'
import KeyManager from './pages/KeyManager'
import SettingsPage from './pages/Settings'

function App() {
  const navigate = useNavigate()

  // Open the user's chosen start menu (once, on launch).
  useEffect(() => {
    const start = loadStartRoute()
    if (start !== '/') navigate(start, { replace: true })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Window starts hidden (tauri.conf) so the window-state plugin can restore
  // size/position first, avoiding the launch jump. Reveal it once React has
  // mounted. NOTE: do NOT use requestAnimationFrame here — WebKit suspends rAF
  // while the window is hidden, so the callback would never fire and the window
  // would stay invisible. useEffect runs regardless of paint/visibility.
  useEffect(() => {
    getCurrentWindow().show().catch(() => {})
  }, [])

  return (
    <TerminalProvider>
      <div
        className="flex flex-col h-screen bg-background text-foreground"
        onDragOver={(e) => {
          // Swallow OS file drags (dragDropEnabled:false routes them here as
          // HTML5 events) so the webview doesn't navigate to the dropped file.
          // Internal pane/tab drags carry 'text/plain'/custom types — untouched.
          if (e.dataTransfer.types.includes('Files')) e.preventDefault()
        }}
        onDrop={(e) => {
          if (e.dataTransfer.types.includes('Files')) e.preventDefault()
        }}
      >
        {/* Dark, draggable strip where the macOS traffic-light buttons sit
            (titleBarStyle: Overlay) — replaces the white native title bar. */}
        <div data-tauri-drag-region className="h-7 shrink-0 bg-background" />

        <div className="flex flex-1 overflow-hidden">
          <Sidebar />

          <main className="flex-1 overflow-auto">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/servers" element={<ServerList />} />
            <Route path="/servers/new" element={<ServerEdit />} />
            <Route path="/servers/:id/edit" element={<ServerEdit />} />
            <Route path="/keys" element={<KeyManager />} />
            {/* Terminal UI is rendered by the always-mounted TerminalHost below */}
            <Route path="/terminal" element={null} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>

            <TerminalHost />
          </main>
        </div>
      </div>
    </TerminalProvider>
  )
}

export default App
