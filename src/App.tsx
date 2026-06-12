import { Routes, Route, Navigate } from 'react-router-dom'
import Sidebar from './components/Sidebar'
import TerminalHost from './components/TerminalHost'
import { TerminalProvider } from './contexts/TerminalContext'
import Dashboard from './pages/Dashboard'
import ServerList from './pages/ServerList'
import ServerEdit from './pages/ServerEdit'
import KeyManager from './pages/KeyManager'
import SettingsPage from './pages/Settings'

function App() {
  return (
    <TerminalProvider>
      <div className="flex h-screen bg-background text-foreground">
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
    </TerminalProvider>
  )
}

export default App
