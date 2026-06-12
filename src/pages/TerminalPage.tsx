import { useState, useEffect, useRef, useCallback } from 'react'
import { useSearchParams } from 'react-router-dom'
import { Terminal as XTerm } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import { WebLinksAddon } from 'xterm-addon-web-links'
import { X, Server } from 'lucide-react'
import 'xterm/css/xterm.css'
import { startSshSession } from '@/lib/tauriCommands'
import type { TerminalTab } from '@/types/terminal'

// Mock server data for demo
const mockServers = [
  { id: 1, name: 'Web Server', host: '192.168.1.100', user: 'user' },
  { id: 2, name: 'Database', host: '10.0.0.50', user: 'admin' },
]

export default function TerminalPage() {
  const [searchParams] = useSearchParams()
  const [tabs, setTabs] = useState<TerminalTab[]>([])
  const [activeTab, setActiveTab] = useState<string | null>(null)
  const [showPasswordModal, setShowPasswordModal] = useState(false)
  const [passwordServerId, setPasswordServerId] = useState<number | null>(null)
  const [password, setPassword] = useState('')

  const terminalRef = useRef<HTMLDivElement>(null)
  const xtermRef = useRef<XTerm | null>(null)
  const fitRef = useRef<FitAddon | null>(null)

  const createTab = useCallback((serverId: number | null = null, serverName: string = '새 탴') => {
    const tabId = `tab-${Date.now()}`
    const newTab: TerminalTab = {
      id: tabId,
      sessionId: null,
      serverId,
      serverName,
    }
    setTabs(prev => [...prev, newTab])
    setActiveTab(tabId)
    return tabId
  }, [])

  const closeTab = useCallback((tabId: string) => {
    setTabs(prev => {
      const newTabs = prev.filter(t => t.id !== tabId)
      if (activeTab === tabId) {
        setActiveTab(newTabs[newTabs.length - 1]?.id || null)
      }
      return newTabs
    })
  }, [activeTab])

  const connectToServer = useCallback(async (serverId: number) => {
    const server = mockServers.find(s => s.id === serverId)
    if (!server) return

    createTab(serverId, `${server.name} - ${server.user}@${server.host}`)

    try {
      const result = await startSshSession(serverId)
      if (result.needsPassword) {
        setPasswordServerId(serverId)
        setShowPasswordModal(true)
      }
    } catch (error) {
      console.error('SSH connection error:', error)
    }
  }, [createTab])

  const handlePasswordSubmit = async () => {
    if (passwordServerId && password) {
      try {
        await startSshSession(passwordServerId, password)
        setShowPasswordModal(false)
        setPassword('')
      } catch (error) {
        console.error('Password authentication failed:', error)
      }
    }
  }

  // Initialize terminal when tab is active
  useEffect(() => {
    if (!terminalRef.current || !activeTab) return

    // Clean up previous terminal
    xtermRef.current?.dispose()

    const term = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
        cursor: '#aeafad',
      },
    })

    const fitAddon = new FitAddon()
    const webLinksAddon = new WebLinksAddon()

    term.loadAddon(fitAddon)
    term.loadAddon(webLinksAddon)
    term.open(terminalRef.current)
    fitAddon.fit()

    xtermRef.current = term
    fitRef.current = fitAddon

    // Auto connect if server is specified in URL
    const serverId = searchParams.get('serverId')
    if (serverId) {
      connectToServer(Number(serverId))
    }

    return () => {
      term.dispose()
    }
  }, [activeTab, searchParams, connectToServer])

  // Fit terminal on window resize
  useEffect(() => {
    const handleResize = () => {
      fitRef.current?.fit()
    }
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  return (
    <div className="flex flex-col h-screen bg-[#1e1e1e]">
      {/* Title bar */}
      <div className="flex items-center h-10 bg-[#2d2d2d] border-b border-[#3e3e3e]">
        <div className="flex items-center px-3 gap-2">
          <div className="flex gap-1.5">
            <div className="w-3 h-3 rounded-full bg-[#ff5f57]"></div>
            <div className="w-3 h-3 rounded-full bg-[#febc2e]"></div>
            <div className="w-3 h-3 rounded-full bg-[#28c840]"></div>
          </div>
        </div>

        <div className="flex-1 flex items-center h-10 overflow-x-auto">
          {tabs.map(tab => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-3 h-9 text-sm border-r border-[#3e3e3e] min-w-[120px] max-w-[200px] ${
                activeTab === tab.id
                  ? 'bg-[#1e1e1e] text-white'
                  : 'bg-[#2d2d2d] text-[#808080] hover:bg-[#333333]'
              }`}
            >
              <Server className="h-3 w-3 flex-shrink-0" />
              <span className="truncate">{tab.serverName}</span>
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  closeTab(tab.id)
                }}
                className="ml-auto p-0.5 rounded hover:bg-[#3e3e3e] flex-shrink-0"
              >
                <X className="h-3 w-3" />
              </button>
            </button>
          ))}

          <button
            onClick={() => createTab()}
            className="px-3 h-9 text-[#808080] hover:text-white hover:bg-[#333333]"
          >
            +
          </button>
        </div>
      </div>

      {/* Toolbar */}
      <div className="flex items-center gap-2 px-4 py-2 bg-[#2d2d2d] border-b border-[#3e3e3e]">
        <select
          onChange={(e) => {
            if (e.target.value) {
              connectToServer(Number(e.target.value))
              e.target.value = ''
            }
          }}
          className="px-3 py-1.5 rounded bg-[#3c3c3c] text-sm border border-[#555] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
        >
          <option value="">서버 선택하여 연결...</option>
          {mockServers.map(server => (
            <option key={server.id} value={server.id}>
              {server.name} ({server.user}@{server.host})
            </option>
          ))}
        </select>
      </div>

      {/* Terminal area */}
      <div className="flex-1 overflow-hidden">
        {tabs.length === 0 ? (
          <div className="flex items-center justify-center h-full text-[#808080]">
            <div className="text-center">
              <Server className="h-12 w-12 mx-auto mb-4 opacity-50" />
              <p>새 탴을 추가하거나 서버를 선택하여 SSH 연결하세요.</p>
            </div>
          </div>
        ) : (
          <div ref={terminalRef} className="h-full" />
        )}
      </div>

      {/* Password modal */}
      {showPasswordModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-full max-w-sm border border-border">
            <h2 className="text-lg font-semibold mb-2">비밀번호 입력</h2>
            <p className="text-sm text-muted-foreground mb-4">
              서버에 연결하려면 비밀번호가 필요합니다.
            </p>
            <input
              type="password"
              value={password}
              onChange={e => setPassword(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handlePasswordSubmit()}
              className="w-full px-3 py-2 rounded-md border border-input bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary mb-4"
              placeholder="비밀번호"
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => {
                  setShowPasswordModal(false)
                  setPassword('')
                }}
                className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
              >
                취소
              </button>
              <button
                onClick={handlePasswordSubmit}
                className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
              >
                연결
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}