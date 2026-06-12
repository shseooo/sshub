import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Plus, Pencil, Trash2, Star, Terminal } from 'lucide-react'
import { useDeleteServer, useServers, useToggleFavorite } from '@/hooks/useServers'
import { useT } from '@/contexts/LanguageContext'

export default function ServerList() {
  const navigate = useNavigate()
  const { t } = useT()

  const { data: servers = [], isLoading } = useServers()
  const deleteServer = useDeleteServer()
  const toggleFavorite = useToggleFavorite()

  const [search, setSearch] = useState('')
  const [group, setGroup] = useState('')

  const groups = useMemo(
    () => [...new Set(servers.map((s) => s.groupName).filter((g): g is string => !!g))].sort(),
    [servers]
  )

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    return servers.filter((s) => {
      if (group && s.groupName !== group) return false
      if (!q) return true
      return [s.name, s.host, s.username, s.groupName ?? '', s.tags ?? '']
        .some((field) => field.toLowerCase().includes(q))
    })
  }, [servers, search, group])

  const handleDelete = (id: number) => {
    if (confirm(t('list.confirmDelete'))) {
      deleteServer.mutate(id)
    }
  }

  return (
    <div className="p-6 max-w-5xl">
      {/* Header */}
      <div className="flex items-end justify-between mb-6 crt-in">
        <div>
          <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
            {t('list.subtitle', { n: servers.length })}
          </p>
          <h1 className="font-display text-5xl leading-none text-foreground">
            SERVERS
          </h1>
        </div>
        <button
          onClick={() => navigate('/servers/new')}
          className="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium"
        >
          <Plus className="h-4 w-4" />
          {t('common.addServer')}
        </button>
      </div>

      {/* Prompt-style search + group filter */}
      <div className="flex gap-2 mb-4 crt-in" style={{ animationDelay: '50ms' }}>
        <div className="relative flex-1">
          <span className="absolute left-3 top-1/2 -translate-y-1/2 text-phosphor text-sm font-semibold select-none">
            ~$
          </span>
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="grep servers..."
            className="w-full pl-9 pr-3 py-2 bg-card border border-border placeholder:text-muted-foreground/50 focus:outline-hidden focus:border-phosphor/60 focus:ring-1 focus:ring-phosphor/40"
          />
        </div>
        {groups.length > 0 && (
          <select
            value={group}
            onChange={(e) => setGroup(e.target.value)}
            className="px-3 py-2 bg-card border border-border focus:outline-hidden focus:border-phosphor/60"
          >
            <option value="">{t('list.allGroups')}</option>
            {groups.map((g) => (
              <option key={g} value={g}>{g}</option>
            ))}
          </select>
        )}
      </div>

      {isLoading ? (
        <div className="flex items-center gap-2 h-32 text-muted-foreground text-sm">
          <span className="animate-blink text-phosphor">▮</span> {t('common.loading')}
        </div>
      ) : filtered.length === 0 ? (
        <div className="bg-card border border-border p-10 text-center crt-in">
          <p className="font-display text-2xl text-muted-foreground mb-1">
            {servers.length === 0 ? 'NO SERVERS REGISTERED' : 'NO MATCHES FOUND'}
          </p>
          <p className="text-xs text-muted-foreground mb-5">
            {servers.length === 0 ? t('list.emptyHintNoServers') : t('list.emptyHintNoMatch')}
          </p>
          {servers.length === 0 && (
            <button
              onClick={() => navigate('/servers/new')}
              className="px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium"
            >
              {t('common.addServer')}
            </button>
          )}
        </div>
      ) : (
        <div className="border border-border divide-y divide-border bg-card">
          {filtered.map((server, i) => (
            <div
              key={server.id}
              className="group flex items-center gap-4 px-4 py-3 hover:bg-accent/60 transition-colors crt-in"
              style={{ animationDelay: `${i * 30}ms` }}
            >
              <span className={server.lastConnectedAt ? 'led shrink-0' : 'led-off shrink-0'} />

              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <h3 className="font-semibold truncate">{server.name}</h3>
                  {server.groupName && (
                    <span className="px-1.5 py-0.5 text-[10px] uppercase tracking-wider border border-border text-muted-foreground">
                      {server.groupName}
                    </span>
                  )}
                </div>
                <p className="text-xs text-muted-foreground truncate">
                  {server.username}@{server.host}
                  <span className="text-muted-foreground/60">:{server.port}</span>
                </p>
              </div>

              <div className="flex items-center gap-1 opacity-70 group-hover:opacity-100 transition-opacity">
                <button
                  onClick={() => navigate(`/terminal?serverId=${server.id}`)}
                  title={t('common.connect')}
                  className="p-2 text-phosphor hover:bg-primary hover:text-primary-foreground transition-colors"
                >
                  <Terminal className="h-4 w-4" />
                </button>
                <button
                  onClick={() => toggleFavorite.mutate(server.id)}
                  title={t('common.favorite')}
                  className="p-2 hover:bg-muted transition-colors"
                >
                  <Star
                    className={`h-4 w-4 ${
                      server.isFavorite
                        ? 'text-phosphor fill-[var(--phosphor)]'
                        : 'text-muted-foreground'
                    }`}
                  />
                </button>
                <button
                  onClick={() => navigate(`/servers/${server.id}/edit`)}
                  title={t('common.edit')}
                  className="p-2 text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                >
                  <Pencil className="h-4 w-4" />
                </button>
                <button
                  onClick={() => handleDelete(server.id)}
                  title={t('common.delete')}
                  className="p-2 text-destructive/80 hover:text-destructive hover:bg-destructive/10 transition-colors"
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
