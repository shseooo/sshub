import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Key, Plus, Trash2, Eye, EyeOff, Copy } from 'lucide-react'
import { getSshKeys, deleteKey } from '@/lib/tauriCommands'
import type { SshKey, KeyType } from '@/types/key'

const keyTypeLabels: Record<KeyType, string> = {
  ed25519: 'Ed25519',
  rsa: 'RSA',
  ecdsa: 'ECDSA',
  dsa: 'DSA',
}

function KeyCard({ keyData }: { keyData: SshKey }) {
  const [isVisible, setIsVisible] = useState(false)
  const queryClient = useQueryClient()

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text)
  }

  const handleDelete = async () => {
    if (confirm(`${keyData.name} 키를 삭제하시겠습니까?`)) {
      await deleteKey(keyData.id!)
      queryClient.invalidateQueries({ queryKey: ['ssh-keys'] })
    }
  }

  return (
    <div className="bg-card border border-border rounded-lg p-4">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2">
          <Key className="h-5 w-5 text-primary" />
          <div>
            <h3 className="font-medium">{keyData.name}</h3>
            <p className="text-xs text-muted-foreground">
              {keyTypeLabels[keyData.keyType]}{keyData.keySize ? ` (${keyData.keySize})` : ''}
              {keyData.passphraseProtected && ' 🔒'}
            </p>
          </div>
        </div>
        <button
          onClick={handleDelete}
          className="p-1.5 rounded-md hover:bg-destructive/10 text-destructive"
        >
          <Trash2 className="h-4 w-4" />
        </button>
      </div>

      <div className="bg-muted/50 rounded-md p-2">
        <div className="flex items-center justify-between mb-1">
          <span className="text-xs text-muted-foreground">공개 키</span>
          <div className="flex gap-1">
            <button
              onClick={() => setIsVisible(!isVisible)}
              className="p-1 rounded hover:bg-muted"
            >
              {isVisible ? <EyeOff className="h-3 w-3" /> : <Eye className="h-3 w-3" />}
            </button>
            <button
              onClick={() => handleCopy(keyData.publicKey)}
              className="p-1 rounded hover:bg-muted"
            >
              <Copy className="h-3 w-3" />
            </button>
          </div>
        </div>
        <p className="text-xs font-mono truncate">
          {isVisible ? keyData.publicKey : '••••••••••••••••••••••••'}
        </p>
      </div>
    </div>
  )
}

export default function KeyManager() {
  const [showCreateDialog, setShowCreateDialog] = useState(false)
  const [showImportDialog, setShowImportDialog] = useState(false)
  const { data: keys = [], isLoading } = useQuery({
    queryKey: ['ssh-keys'],
    queryFn: getSshKeys,
  })

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold">SSH 키 관리</h1>
        <div className="flex gap-2">
          <button
            onClick={() => setShowCreateDialog(true)}
            className="flex items-center gap-2 px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
          >
            <Plus className="h-4 w-4" />
            키 생성
          </button>
          <button
            onClick={() => setShowImportDialog(true)}
            className="flex items-center gap-2 px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
          >
            <Key className="h-4 w-4" />
            키 수입
          </button>
        </div>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center h-32 text-muted-foreground">
          로딩 중...
        </div>
      ) : keys.length === 0 ? (
        <div className="bg-card border border-border rounded-lg p-8 text-center">
          <Key className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
          <p className="text-muted-foreground mb-4">
            등록된 SSH 키가 없습니다.
          </p>
          <div className="flex justify-center gap-3">
            <button
              onClick={() => setShowCreateDialog(true)}
              className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
            >
              키 생성
            </button>
            <button
              onClick={() => setShowImportDialog(true)}
              className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
            >
              키 수입
            </button>
          </div>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {keys.map((key) => (
            <KeyCard key={key.id} keyData={key} />
          ))}
        </div>
      )}

      {/* 키 생성 모달 (placeholder) */}
      {showCreateDialog && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-full max-w-md">
            <h2 className="text-lg font-semibold mb-4">SSH 키 생성</h2>
            <p className="text-muted-foreground mb-4">
              키 생성 기능은 곧 제공됩니다.
            </p>
            <div className="flex justify-end">
              <button
                onClick={() => setShowCreateDialog(false)}
                className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
              >
                닫기
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 키 수입 모달 (placeholder) */}
      {showImportDialog && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card rounded-lg p-6 w-full max-w-md">
            <h2 className="text-lg font-semibold mb-4">SSH 키 수입</h2>
            <p className="text-muted-foreground mb-4">
              키 수입 기능은 곧 제공됩니다.
            </p>
            <div className="flex justify-end">
              <button
                onClick={() => setShowImportDialog(false)}
                className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
              >
                닫기
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}