import { Server, Star, Plus, Terminal } from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { getServers } from '@/lib/tauriCommands'
import { Link } from 'react-router-dom'
import type { Server as ServerType } from '@/types/server'

function ServerCard({ server }: { server: ServerType }) {
  return (
    <div className="bg-card border border-border rounded-lg p-4 hover:border-primary/50 transition-colors">
      <div className="flex items-start justify-between mb-2">
        <div className="flex items-center gap-2">
          <Server className="h-4 w-4 text-muted-foreground" />
          <h3 className="font-medium">{server.name}</h3>
        </div>
        {server.isFavorite && <Star className="h-4 w-4 text-yellow-500 fill-yellow-500" />}
      </div>
      <div className="text-sm text-muted-foreground space-y-1">
        <p>{server.host}:{server.port}</p>
        <p>{server.username}@{server.host}</p>
      </div>
      <div className="flex gap-2 mt-3">
        <Link
          to={`/terminal?serverId=${server.id}`}
          className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-primary text-primary-foreground text-sm hover:bg-primary/90"
        >
          <Terminal className="h-3 w-3" />
          연결
        </Link>
        <Link
          to={`/servers/${server.id}/edit`}
          className="flex items-center gap-1 px-3 py-1.5 rounded-md bg-secondary text-secondary-foreground text-sm hover:bg-secondary/80"
        >
          편집
        </Link>
      </div>
    </div>
  )
}

export default function Dashboard() {
  const { data: servers = [], isLoading } = useQuery({
    queryKey: ['servers'],
    queryFn: getServers,
  })

  const favoriteServers = servers.filter(s => s.isFavorite)
  const recentServers = servers.slice(0, 6)

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">대시보드</h1>
        <Link
          to="/servers/new"
          className="flex items-center gap-2 px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Plus className="h-4 w-4" />
          새 서버
        </Link>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center h-32 text-muted-foreground">
          로딩 중...
        </div>
      ) : (
        <>
          {favoriteServers.length > 0 && (
            <section className="mb-8">
              <h2 className="text-lg font-semibold mb-4 flex items-center gap-2">
                <Star className="h-4 w-4 text-yellow-500 fill-yellow-500" />
                즐겨찾기
              </h2>
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                {favoriteServers.map(server => (
                  <ServerCard key={server.id} server={server} />
                ))}
              </div>
            </section>
          )}

          <section>
            <h2 className="text-lg font-semibold mb-4">서버 목록</h2>
            {servers.length === 0 ? (
              <div className="bg-card border border-border rounded-lg p-8 text-center">
                <Server className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
                <p className="text-muted-foreground mb-4">
                  등록된 서버가 없습니다.
                </p>
                <Link
                  to="/servers/new"
                  className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
                >
                  <Plus className="h-4 w-4" />
                  서버 추가하기
                </Link>
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                {recentServers.map(server => (
                  <ServerCard key={server.id} server={server} />
                ))}
              </div>
            )}
          </section>
        </>
      )}
    </div>
  )
}