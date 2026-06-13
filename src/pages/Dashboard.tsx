import { Star, Plus, Terminal, Pencil } from 'lucide-react'
import { Link } from 'react-router-dom'
import { useServers } from '@/hooks/useServers'
import { useT } from '@/contexts/LanguageContext'
import type { Server as ServerType } from '@/types/server'

function ServerCard({ server, index }: { server: ServerType; index: number }) {
  const { t } = useT()
  return (
    <div
      className="bracket group bg-card border border-border hover:border-phosphor/50 transition-colors p-4 crt-in"
      style={{ animationDelay: `${index * 45}ms` }}
    >
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2.5 min-w-0">
          <span className={server.lastConnectedAt ? 'led shrink-0' : 'led-off shrink-0'} />
          <h3 className="font-semibold truncate">{server.name}</h3>
        </div>
        {server.isFavorite && (
          <Star className="h-3.5 w-3.5 text-phosphor fill-[var(--phosphor)] shrink-0" />
        )}
      </div>

      <p className="text-xs text-muted-foreground mb-1 truncate">
        <span className="text-phosphor/70">$</span> ssh {server.username}@{server.host}
        {server.port !== 22 && ` -p ${server.port}`}
      </p>
      <p className="text-[10px] text-muted-foreground/70 uppercase tracking-wider">
        {server.groupName ? `group/${server.groupName}` : 'ungrouped'}
        {server.lastConnectedAt &&
          ` · last ${new Date(server.lastConnectedAt).toLocaleDateString()}`}
      </p>

      <div className="flex gap-2 mt-4">
        <Link
          to={`/terminal?serverId=${server.id}`}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium border border-phosphor/40 text-phosphor hover:bg-primary hover:text-primary-foreground hover:border-transparent transition-colors"
        >
          <Terminal className="h-3 w-3" />
          {t('common.connect')}
        </Link>
        <Link
          to={`/servers/${server.id}/edit`}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs border border-border text-muted-foreground hover:text-foreground hover:border-muted-foreground transition-colors"
        >
          <Pencil className="h-3 w-3" />
          {t('common.edit')}
        </Link>
      </div>
    </div>
  )
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex-1 bg-card border border-border px-4 py-3">
      <p className="font-display text-3xl leading-none text-phosphor glow-text tabular-nums">
        {String(value).padStart(2, '0')}
      </p>
      <p className="mt-1 text-[9px] tracking-[0.25em] uppercase text-muted-foreground">{label}</p>
    </div>
  )
}

export default function Dashboard() {
  const { t } = useT()
  const { data: servers = [], isLoading } = useServers()

  const favoriteServers = servers.filter((s) => s.isFavorite)
  const recentServers = servers
    .filter((s) => s.lastConnectedAt)
    .sort((a, b) => (b.lastConnectedAt! > a.lastConnectedAt! ? 1 : -1))
    .slice(0, 6)

  return (
    <div className="p-6 max-w-6xl">
      {/* Header */}
      <div className="flex items-end justify-between mb-6 crt-in">
        <div>
          <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
            ~/dashboard
          </p>
          <h1 className="font-display text-5xl leading-none text-foreground">
            DASHBOARD
          </h1>
        </div>
        <Link
          to="/servers/new"
          className="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium"
        >
          <Plus className="h-4 w-4" />
          {t('dashboard.newServer')}
        </Link>
      </div>

      {/* Stats */}
      <div className="flex gap-3 mb-8 crt-in" style={{ animationDelay: '60ms' }}>
        <Stat label="Servers" value={servers.length} />
        <Stat label="Favorites" value={favoriteServers.length} />
        <Stat label="Recent" value={recentServers.length} />
      </div>

      {isLoading ? (
        <div className="flex items-center gap-2 h-32 text-muted-foreground text-sm">
          <span className="animate-blink text-phosphor">▮</span> {t('common.loading')}
        </div>
      ) : (
        <>
          {favoriteServers.length > 0 && (
            <section className="mb-8">
              <h2 className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-3 flex items-center gap-2">
                <Star className="h-3 w-3 text-phosphor" /> Favorites
                <span className="flex-1 border-t border-border" />
              </h2>
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                {favoriteServers.map((server, i) => (
                  <ServerCard key={server.id} server={server} index={i} />
                ))}
              </div>
            </section>
          )}

          {recentServers.length > 0 && (
            <section className="mb-8">
              <h2 className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-3 flex items-center gap-2">
                <Terminal className="h-3 w-3 text-phosphor" /> Recent Connections
                <span className="flex-1 border-t border-border" />
              </h2>
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                {recentServers.map((server, i) => (
                  <ServerCard key={server.id} server={server} index={i} />
                ))}
              </div>
            </section>
          )}

          {servers.length === 0 && (
            <div className="bg-card border border-border p-10 text-center crt-in">
              <p className="font-display text-2xl text-muted-foreground mb-1">NO SERVERS REGISTERED</p>
              <p className="text-xs text-muted-foreground mb-5">{t('dashboard.emptyHint')}</p>
              <Link
                to="/servers/new"
                className="inline-flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium"
              >
                <Plus className="h-4 w-4" />
                {t('dashboard.addServerCta')}
              </Link>
            </div>
          )}
        </>
      )}
    </div>
  )
}
