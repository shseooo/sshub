import { Home, Server, Key, Settings, Plus } from 'lucide-react'
import { Link, useLocation } from 'react-router-dom'
import { cn } from '@/lib/utils'

const navItems = [
  { icon: Home, label: '대시보드', path: '/' },
  { icon: Server, label: '서버 목록', path: '/servers' },
  { icon: Key, label: 'SSH 키 관리', path: '/keys' },
  { icon: Settings, label: '설정', path: '/settings' },
]

interface SidebarProps {
  onNewServer: () => void
}

export default function Sidebar({ onNewServer }: SidebarProps) {
  const location = useLocation()

  return (
    <aside className="w-64 bg-card border-r border-border flex flex-col h-full">
      <div className="p-4 border-b border-border">
        <div className="flex items-center gap-2">
          <Server className="h-6 w-6 text-primary" />
          <h1 className="text-lg font-bold">Connectunnel</h1>
        </div>
      </div>

      <nav className="flex-1 p-2 space-y-1">
        {navItems.map((item) => {
          const Icon = item.icon
          const isActive = location.pathname === item.path
          return (
            <Link
              key={item.path}
              to={item.path}
              className={cn(
                'flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors',
                isActive
                  ? 'bg-accent text-accent-foreground'
                  : 'hover:bg-muted text-muted-foreground hover:text-foreground'
              )}
            >
              <Icon className="h-4 w-4" />
              {item.label}
            </Link>
          )
        })}
      </nav>

      <div className="p-2 border-t border-border">
        <button
          onClick={onNewServer}
          className="w-full flex items-center gap-2 px-3 py-2 rounded-md text-sm bg-primary text-primary-foreground hover:bg-primary/90 transition-colors"
        >
          <Plus className="h-4 w-4" />
          서버 추가
        </button>
      </div>
    </aside>
  )
}