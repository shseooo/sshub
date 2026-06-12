import { useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { homeDir } from '@tauri-apps/api/path'
import { Key, Plus, Trash2, Eye, EyeOff, Copy, X, FolderOpen, FileWarning } from 'lucide-react'
import { useCreateSshKey, useDeleteSshKey, useImportSshKey, useSshKeys } from '@/hooks/useKeys'
import { loadKeyFile } from '@/lib/tauriCommands'
import { useT } from '@/contexts/LanguageContext'
import type { SshKey, KeyType } from '@/types/key'

function detectKeyType(publicKey: string): KeyType | null {
  if (publicKey.startsWith('ssh-ed25519')) return 'ed25519'
  if (publicKey.startsWith('ssh-rsa')) return 'rsa'
  if (publicKey.startsWith('ecdsa-')) return 'ecdsa'
  if (publicKey.startsWith('ssh-dss')) return 'dsa'
  return null
}

const keyTypeLabels: Record<KeyType, string> = {
  ed25519: 'Ed25519',
  rsa: 'RSA',
  ecdsa: 'ECDSA',
  dsa: 'DSA',
}

const inputClass =
  'w-full px-3 py-2 rounded-md bg-background border border-border text-sm focus:outline-hidden focus:ring-2 focus:ring-primary'

function ModalShell({
  title,
  onClose,
  children,
}: {
  title: string
  onClose: () => void
  children: React.ReactNode
}) {
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-card rounded-lg p-6 w-full max-w-md border border-border max-h-[85vh] overflow-y-auto">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">{title}</h2>
          <button onClick={onClose} className="p-1 rounded hover:bg-muted">
            <X className="h-4 w-4" />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}

function CreateKeyDialog({ onClose }: { onClose: () => void }) {
  const { t } = useT()
  const createKey = useCreateSshKey()
  const [name, setName] = useState('')
  const [keyType, setKeyType] = useState<KeyType>('ed25519')
  const [keySize, setKeySize] = useState(3072)
  const [passphrase, setPassphrase] = useState('')

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    createKey.mutate(
      {
        name: name.trim(),
        keyType,
        keySize: keyType === 'rsa' ? keySize : undefined,
        passphrase: passphrase || undefined,
      },
      { onSuccess: onClose }
    )
  }

  return (
    <ModalShell title={t('keys.createTitle')} onClose={onClose}>
      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.name')}</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className={inputClass}
            placeholder={t('keys.namePlaceholderCreate')}
            required
            autoFocus
          />
          <p className="text-xs text-muted-foreground mt-1">{t('keys.createNameHint')}</p>
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.keyType')}</label>
          <select
            value={keyType}
            onChange={(e) => setKeyType(e.target.value as KeyType)}
            className={inputClass}
          >
            <option value="ed25519">{t('keys.ed25519Rec')}</option>
            <option value="rsa">RSA</option>
            <option value="ecdsa">ECDSA</option>
          </select>
        </div>

        {keyType === 'rsa' && (
          <div>
            <label className="block text-sm font-medium mb-1">{t('keys.keySize')}</label>
            <select
              value={keySize}
              onChange={(e) => setKeySize(Number(e.target.value))}
              className={inputClass}
            >
              <option value={2048}>2048</option>
              <option value={3072}>3072</option>
              <option value={4096}>4096</option>
            </select>
          </div>
        )}

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.passphraseOpt')}</label>
          <input
            type="password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            className={inputClass}
            placeholder={t('keys.passphraseCreatePlaceholder')}
          />
        </div>

        {createKey.isError && (
          <p className="text-sm text-destructive break-all">{String(createKey.error)}</p>
        )}

        <div className="flex justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
          >
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            disabled={createKey.isPending || !name.trim()}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {createKey.isPending ? t('keys.creating') : t('keys.create')}
          </button>
        </div>
      </form>
    </ModalShell>
  )
}

function ImportKeyDialog({ onClose }: { onClose: () => void }) {
  const { t } = useT()
  const importKey = useImportSshKey()
  const [name, setName] = useState('')
  const [keyType, setKeyType] = useState<KeyType>('ed25519')
  const [publicKey, setPublicKey] = useState('')
  const [privateKey, setPrivateKey] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const [loadError, setLoadError] = useState<string | null>(null)

  const handleLoadFile = async () => {
    setLoadError(null)
    let defaultPath: string | undefined
    try {
      defaultPath = `${await homeDir()}/.ssh`
    } catch {
      defaultPath = undefined
    }

    const path = await open({
      multiple: false,
      directory: false,
      title: t('keys.dialogPickTitle'),
      defaultPath,
    })
    if (typeof path !== 'string') return

    try {
      const loaded = await loadKeyFile(path)
      if (loaded.publicKey) {
        setPublicKey(loaded.publicKey)
        const detected = detectKeyType(loaded.publicKey)
        if (detected) setKeyType(detected)
      }
      if (loaded.privateKey) setPrivateKey(loaded.privateKey)
      if (loaded.fileName) setName((prev) => prev.trim() || loaded.fileName)
    } catch (err) {
      setLoadError(t('keys.loadError', { err: String(err) }))
    }
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    importKey.mutate(
      {
        name: name.trim(),
        keyType,
        publicKey: publicKey.trim(),
        pemData: privateKey.trim() || undefined,
        passphrase: passphrase || undefined,
      },
      { onSuccess: onClose }
    )
  }

  return (
    <ModalShell title={t('keys.importTitle')} onClose={onClose}>
      <form onSubmit={handleSubmit} className="space-y-4">
        <button
          type="button"
          onClick={handleLoadFile}
          className="w-full flex items-center justify-center gap-2 px-3 py-2 border border-phosphor/40 text-phosphor hover:bg-primary hover:text-primary-foreground hover:border-transparent transition-colors text-sm"
        >
          <FolderOpen className="h-4 w-4" />
          {t('keys.loadFromFile')}
        </button>
        <p className="text-xs text-muted-foreground -mt-2">{t('keys.loadFromFileHint')}</p>
        {loadError && <p className="text-sm text-destructive break-all">{loadError}</p>}

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.name')}</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className={inputClass}
            placeholder={t('keys.namePlaceholderImport')}
            required
            autoFocus
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.keyType')}</label>
          <select
            value={keyType}
            onChange={(e) => setKeyType(e.target.value as KeyType)}
            className={inputClass}
          >
            <option value="ed25519">Ed25519</option>
            <option value="rsa">RSA</option>
            <option value="ecdsa">ECDSA</option>
            <option value="dsa">DSA</option>
          </select>
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.publicKeyLabel')}</label>
          <textarea
            value={publicKey}
            onChange={(e) => setPublicKey(e.target.value)}
            className={`${inputClass} font-mono h-20 resize-none`}
            placeholder="ssh-ed25519 AAAA... user@host"
            required
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.privateKeyOpt')}</label>
          <textarea
            value={privateKey}
            onChange={(e) => setPrivateKey(e.target.value)}
            className={`${inputClass} font-mono h-28 resize-none`}
            placeholder={'-----BEGIN OPENSSH PRIVATE KEY-----\n...'}
          />
          <p className="text-xs text-muted-foreground mt-1">{t('keys.privateKeyHint')}</p>
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.passphraseOpt')}</label>
          <input
            type="password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            className={inputClass}
            placeholder={t('keys.passphraseImportPlaceholder')}
          />
        </div>

        {importKey.isError && (
          <p className="text-sm text-destructive break-all">{String(importKey.error)}</p>
        )}

        <div className="flex justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
          >
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            disabled={importKey.isPending || !name.trim() || !publicKey.trim()}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {importKey.isPending ? t('keys.importing') : t('keys.import')}
          </button>
        </div>
      </form>
    </ModalShell>
  )
}

function KeyCard({ keyData }: { keyData: SshKey }) {
  const { t } = useT()
  const [isVisible, setIsVisible] = useState(false)
  const deleteKey = useDeleteSshKey()

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text)
  }

  const handleDelete = () => {
    if (confirm(t('keys.confirmDelete', { name: keyData.name }))) {
      deleteKey.mutate(keyData.id)
    }
  }

  return (
    <div className="bracket bg-card border border-border hover:border-phosphor/50 transition-colors p-4 crt-in">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2">
          <Key className="h-5 w-5 text-phosphor" />
          <div>
            <h3 className="font-medium">{keyData.name}</h3>
            <p className="text-xs text-muted-foreground">
              {keyTypeLabels[keyData.keyType]}{keyData.keySize ? ` (${keyData.keySize})` : ''}
              {keyData.passphraseProtected && ' 🔒'}
            </p>
          </div>
        </div>
        {keyData.hasPrivateFile === false && (
          <span
            title={t('keys.missingTitle')}
            className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] uppercase tracking-wider border border-destructive/40 text-destructive shrink-0"
          >
            <FileWarning className="h-3 w-3" />
            {t('keys.missingBadge')}
          </span>
        )}
        <button
          onClick={handleDelete}
          className="p-1.5 rounded-md hover:bg-destructive/10 text-destructive"
        >
          <Trash2 className="h-4 w-4" />
        </button>
      </div>

      <div className="bg-muted/50 rounded-md p-2">
        <div className="flex items-center justify-between mb-1">
          <span className="text-xs text-muted-foreground">{t('keys.publicKey')}</span>
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
  const { t } = useT()
  const [showCreateDialog, setShowCreateDialog] = useState(false)
  const [showImportDialog, setShowImportDialog] = useState(false)
  const { data: keys = [], isLoading } = useSshKeys()

  return (
    <div className="p-6 max-w-5xl">
      <div className="flex items-end justify-between mb-6 crt-in">
        <div>
          <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
            {t('keys.subtitle', { n: keys.length })}
          </p>
          <h1 className="font-display text-5xl leading-none text-foreground">
            SSH KEYS<span className="text-phosphor animate-blink">▮</span>
          </h1>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setShowCreateDialog(true)}
            className="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium"
          >
            <Plus className="h-4 w-4" />
            {t('keys.create')}
          </button>
          <button
            onClick={() => setShowImportDialog(true)}
            className="flex items-center gap-2 px-4 py-2 border border-border text-muted-foreground hover:text-foreground hover:border-muted-foreground transition-colors text-sm"
          >
            <Key className="h-4 w-4" />
            {t('keys.import')}
          </button>
        </div>
      </div>

      {isLoading ? (
        <div className="flex items-center justify-center h-32 text-muted-foreground">
          {t('common.loading')}
        </div>
      ) : keys.length === 0 ? (
        <div className="bg-card border border-border rounded-lg p-8 text-center">
          <Key className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
          <p className="text-muted-foreground mb-4">{t('keys.empty')}</p>
          <div className="flex justify-center gap-3">
            <button
              onClick={() => setShowCreateDialog(true)}
              className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90"
            >
              {t('keys.create')}
            </button>
            <button
              onClick={() => setShowImportDialog(true)}
              className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80"
            >
              {t('keys.import')}
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

      {showCreateDialog && <CreateKeyDialog onClose={() => setShowCreateDialog(false)} />}
      {showImportDialog && <ImportKeyDialog onClose={() => setShowImportDialog(false)} />}
    </div>
  )
}
