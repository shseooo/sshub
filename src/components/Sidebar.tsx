import { useState } from 'react'
import {
  Home,
  Server,
  Key,
  Settings,
  SquareTerminal,
  Plus,
  PanelLeftClose,
  PanelLeftOpen,
} from 'lucide-react'
import { Link, useLocation } from 'react-router-dom'
import { cn } from '@/lib/utils'
import { useT } from '@/contexts/LanguageContext'

const navItems = [
  { icon: Home, key: 'nav.dashboard', path: '/' },
  { icon: Server, key: 'nav.servers', path: '/servers' },
  { icon: SquareTerminal, key: 'nav.terminal', path: '/terminal' },
  { icon: Key, key: 'nav.keys', path: '/keys' },
  { icon: Settings, key: 'nav.settings', path: '/settings' },
]

export default function Sidebar() {
  const location = useLocation()
  const { t } = useT()
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem('sidebar-collapsed') === '1'
  )

  const toggle = () => {
    setCollapsed((prev) => {
      const next = !prev
      localStorage.setItem('sidebar-collapsed', next ? '1' : '0')
      return next
    })
  }

  return (
    <aside
      className={cn(
        'bg-card border-r border-border flex flex-col h-full transition-[width] duration-150',
        collapsed ? 'w-14' : 'w-60'
      )}
    >
      {/* Logo + collapse toggle */}
      <div
        className={cn(
          'border-b border-border flex',
          collapsed
            ? 'flex-col items-center gap-2 py-3'
            : 'items-start justify-between px-4 pt-5 pb-4'
        )}
      >
        {collapsed ? (
          <span className="font-display text-2xl leading-none text-phosphor glow-text select-none">
            s<span className="animate-blink">_</span>
          </span>
        ) : (
          <div>
            <h1 className="font-display text-4xl leading-none text-phosphor glow-text select-none">
              sshub<span className="animate-blink">_</span>
            </h1>
            <p className="mt-1.5 text-[9px] tracking-[0.32em] uppercase text-muted-foreground">
              SSH Ops Console
            </p>
          </div>
        )}
        <button
          onClick={toggle}
          title={collapsed ? t('sidebar.expand') : t('sidebar.collapse')}
          aria-label={collapsed ? t('sidebar.expand') : t('sidebar.collapse')}
          className="p-1.5 text-muted-foreground hover:text-phosphor hover:bg-muted transition-colors"
        >
          {collapsed ? (
            <PanelLeftOpen className="h-4 w-4" />
          ) : (
            <PanelLeftClose className="h-4 w-4" />
          )}
        </button>
      </div>

      {/* Nav */}
      <nav className="flex-1 py-3">
        {navItems.map((item, i) => {
          const Icon = item.icon
          const isActive = location.pathname === item.path
          return (
            <Link
              key={item.path}
              to={item.path}
              title={collapsed ? t(item.key) : undefined}
              className={cn(
                'group flex items-center text-sm border-l-2 transition-colors',
                collapsed ? 'justify-center px-0 py-3' : 'gap-3 px-4 py-2.5',
                isActive
                  ? 'border-phosphor bg-accent text-accent-foreground'
                  : 'border-transparent text-muted-foreground hover:text-foreground hover:bg-muted'
              )}
            >
              {!collapsed && (
                <span
                  className={cn(
                    'text-[10px] tabular-nums',
                    isActive ? 'text-phosphor' : 'text-muted-foreground/60'
                  )}
                >
                  0{i + 1}
                </span>
              )}
              <Icon className={cn('h-4 w-4 shrink-0', isActive && 'text-phosphor')} />
              {!collapsed && (
                <>
                  <span className="flex-1">{t(item.key)}</span>
                  {isActive && <span className="text-phosphor text-xs">▸</span>}
                </>
              )}
            </Link>
          )
        })}
      </nav>

      {/* Footer */}
      <div className={cn('border-t border-border', collapsed ? 'p-2' : 'p-3 space-y-3')}>
        <Link
          to="/servers/new"
          title={collapsed ? t('common.addServer') : undefined}
          className={cn(
            'flex items-center justify-center bg-primary text-primary-foreground hover:bg-phosphor transition-colors font-medium',
            collapsed ? 'p-2' : 'w-full gap-2 px-3 py-2 text-sm'
          )}
        >
          <Plus className="h-4 w-4 shrink-0" />
          {!collapsed && t('common.addServer')}
        </Link>
        <div
          className={cn(
            'flex items-center',
            collapsed ? 'justify-center pt-2' : 'gap-2 px-1'
          )}
        >
          <span className="led" />
          {!collapsed && (
            <span className="text-[9px] tracking-[0.25em] uppercase text-muted-foreground">
              System Online
            </span>
          )}
        </div>
      </div>
    </aside>
  )
}
