import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  createServer,
  deleteServer,
  getServerById,
  getServers,
  toggleFavorite,
  updateServer,
} from '@/lib/tauriCommands'
import type { CreateServerDto, UpdateServerDto } from '@/types/server'

export const serverKeys = {
  all: ['servers'] as const,
  detail: (id: number) => ['servers', id] as const,
}

export function useServers() {
  return useQuery({
    queryKey: serverKeys.all,
    queryFn: getServers,
  })
}

export function useServer(id: number | undefined) {
  return useQuery({
    queryKey: serverKeys.detail(id ?? -1),
    queryFn: () => getServerById(id!),
    enabled: id != null,
  })
}

export function useSaveServer() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (dto: CreateServerDto | UpdateServerDto) =>
      'id' in dto ? updateServer(dto) : createServer(dto),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: serverKeys.all }),
  })
}

export function useDeleteServer() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: deleteServer,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: serverKeys.all }),
  })
}

export function useToggleFavorite() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: toggleFavorite,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: serverKeys.all }),
  })
}
