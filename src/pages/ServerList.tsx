import { useNavigate } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus, Pencil, Trash2, Star } from 'lucide-react'
import { getServers, deleteServer, toggleFavorite } from '@/lib/tauriCommands'
import { Server as ServerIcon } from 'lucide-react'

export default function ServerList() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()

  const { data: servers = [], isLoading } = useQuery({
    queryKey: ['servers'],
    queryFn: getServers,
  })

  const handleDelete = async (id: number) => {
    if (confirm('정말 이 서버를 삭제하시겠습니까?')) {
      await deleteServer(id)
      queryClient.invalidateQueries({ queryKey: ['servers'] })
    }
  }

  const handleToggleFavorite = async (id: number) => {
    await toggleFavorite(id)
    queryClient.invalidateQueries({ queryKey: ['servers'] })
  }

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">서버 목록</h1>
        <button
          onClick={() => navigate('/servers/new')}
          className="flex items-center gap-2 px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Plus className="h-4 w-4" />
          서버 추가
        </button>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center h-32 text-muted-foreground">
          로딩 중...
        </div>
      ) : servers.length === 0 ? (
        <div className="bg-card border border-border rounded-lg p-8 text-center">
          <ServerIcon className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
          <p className="text-muted-foreground mb-4">
            등록된 서버가 없습니다.
          </p>
          <button
            onClick={() => navigate('/servers/new')}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
          >
            서버 추가
          </button>
        </div>
      ) : (
        <div className="space-y-2">
          {servers.map((server) => (
            <div
              key={server.id}
              className="bg-card border border-border rounded-lg p-4 flex items-center justify-between hover:border-primary/50 transition-colors"
            >
              <div className="flex items-center gap-4">
                <ServerIcon className="h-5 w-5 text-muted-foreground" />
                <div>
                  <div className="flex items-center gap-2">
                    <h3 className="font-medium">{server.name}</h3>
                    {server.isFavorite && (
                      <Star className="h-3 w-3 text-yellow-500 fill-yellow-500" />
                    )}
                  </div>
                  <p className="text-sm text-muted-foreground">
                    {server.host}:{server.port} ({server.username})
                  </p>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => handleToggleFavorite(server.id!)}
                  className="p-1.5 rounded-md hover:bg-muted"
                >
                  <Star className="h-4 w-4" />
                </button>
                <button
                  onClick={() => navigate(`/servers/${server.id}`)}
                  className="p-1.5 rounded-md hover:bg-muted"
                >
                  <Pencil className="h-4 w-4 text-muted-foreground" />
                </button>
                <button
                  onClick={() => handleDelete(server.id!)}
                  className="p-1.5 rounded-md hover:bg-destructive/10 text-destructive"
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