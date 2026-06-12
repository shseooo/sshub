import { useState, useEffect } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { getServerById, createServer, updateServer } from '@/lib/tauriCommands'
import type { Server } from '@/types/server'

export default function ServerEdit() {
  const { id } = useParams<{ id?: string }>()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const isEdit = !!id

  const { data: server, isLoading } = useQuery({
    queryKey: ['server', id],
    queryFn: () => id ? getServerById(Number(id)) : null,
    enabled: isEdit,
  })

  const [name, setName] = useState('')
  const [host, setHost] = useState('')
  const [port, setPort] = useState(22)
  const [username, setUsername] = useState('')
  const [authType, setAuthType] = useState<'key' | 'password'>('key')

  useEffect(() => {
    if (server) {
      setName(server.name)
      setHost(server.host)
      setPort(server.port)
      setUsername(server.username)
      setAuthType(server.authType as 'key' | 'password')
    }
  }, [server])

  const mutation = useMutation({
    mutationFn: async (data: Partial<Server>) => {
      if (isEdit && id) {
        return updateServer({ ...data, id: Number(id) })
      }
      return createServer(data)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['servers'] })
      navigate('/servers')
    },
  })

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    mutation.mutate({
      name,
      host,
      port,
      username,
      authType,
    })
  }

  if (isEdit && isLoading) {
    return <div className="p-6">로딩 중...</div>
  }

  return (
    <div className="p-6 max-w-2xl">
      <h1 className="text-2xl font-bold mb-6">
        {isEdit ? '서버 수정' : '서버 추가'}
      </h1>

      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <label className="block text-sm font-medium mb-1">이름</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full px-3 py-2 rounded-md bg-background border border-border"
            required
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">호스트</label>
          <input
            type="text"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            className="w-full px-3 py-2 rounded-md bg-background border border-border"
            required
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">포트</label>
          <input
            type="number"
            value={port}
            onChange={(e) => setPort(Number(e.target.value))}
            className="w-full px-3 py-2 rounded-md bg-background border border-border"
            min={1}
            max={65535}
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">사용자</label>
          <input
            type="text"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            className="w-full px-3 py-2 rounded-md bg-background border border-border"
            required
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">인증 방식</label>
          <select
            value={authType}
            onChange={(e) => setAuthType(e.target.value as 'key' | 'password')}
            className="w-full px-3 py-2 rounded-md bg-background border border-border"
          >
            <option value="key">SSH 키</option>
            <option value="password">비밀번호</option>
          </select>
        </div>

        <div className="flex gap-3 pt-4">
          <button
            type="submit"
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
          >
            저장
          </button>
          <button
            type="button"
            onClick={() => navigate('/servers')}
            className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
          >
            취소
          </button>
        </div>
      </form>
    </div>
  )
}