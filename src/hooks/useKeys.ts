import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createSshKey, deleteKey, getSshKeys, importSshKey, updateSshKey } from '@/lib/tauriCommands'

export const sshKeyKeys = {
  all: ['ssh-keys'] as const,
}

export function useSshKeys() {
  return useQuery({
    queryKey: sshKeyKeys.all,
    queryFn: getSshKeys,
  })
}

export function useCreateSshKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: createSshKey,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sshKeyKeys.all }),
  })
}

export function useImportSshKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: importSshKey,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sshKeyKeys.all }),
  })
}

export function useUpdateSshKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: updateSshKey,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sshKeyKeys.all }),
  })
}

export function useDeleteSshKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: deleteKey,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: sshKeyKeys.all }),
  })
}
