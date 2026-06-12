import { useState } from 'react'
import { Routes, Route, Navigate } from 'react-router-dom'
import Sidebar from './components/Sidebar'
import Dashboard from './pages/Dashboard'
import ServerList from './pages/ServerList'
import ServerEdit from './pages/ServerEdit'
import KeyManager from './pages/KeyManager'
import TerminalPage from './pages/TerminalPage'
import SettingsPage from './pages/Settings'

function App() {
  const [showNewServerDialog, setShowNewServerDialog] = useState(false)

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar onNewServer={() => setShowNewServerDialog(true)} />
      
      <main className="flex-1 overflow-auto">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/servers" element={<ServerList />} />
          <Route path="/servers/new" element={<ServerEdit />} />
          <Route path="/servers/:id/edit" element={<ServerEdit />} />
          <Route path="/keys" element={<KeyManager />} />
          <Route path="/terminal" element={<TerminalPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </main>

      {showNewServerDialog && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-full max-w-md">
            <h2 className="text-lg font-semibold mb-4">새 서버 추가</h2>
            <p className="text-muted-foreground mb-4">
              서버 추가 기능은 곧 제공됩니다.
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowNewServerDialog(false)}
                className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
              >
                닫기
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

export default App