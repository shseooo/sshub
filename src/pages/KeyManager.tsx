import { useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { homeDir } from '@tauri-apps/api/path'
import { Key, Plus, Trash2, Eye, EyeOff, Copy, X, FolderOpen, FileWarning, KeyRound, Pencil } from 'lucide-react'
import { useQueryClient } from '@tanstack/react-query'
import { useCreateSshKey, useDeleteSshKey, useImportSshKey, useUpdateSshKey, useSshKeys, sshKeyKeys } from '@/hooks/useKeys'
import { loadKeyFile, derivePublicKeyFromPem, changeKeyPassphrase } from '@/lib/tauriCommands'
import { useT } from '@/contexts/LanguageContext'
import { Select } from '@/components/Select'
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
          <Select
            value={keyType}
            onChange={(v) => setKeyType(v as KeyType)}
            ariaLabel={t('keys.keyType')}
            options={[
              { value: 'ed25519', label: t('keys.ed25519Rec') },
              { value: 'rsa', label: 'RSA' },
              { value: 'ecdsa', label: 'ECDSA' },
            ]}
          />
        </div>

        {keyType === 'rsa' && (
          <div>
            <label className="block text-sm font-medium mb-1">{t('keys.keySize')}</label>
            <Select
              value={String(keySize)}
              onChange={(v) => setKeySize(Number(v))}
              ariaLabel={t('keys.keySize')}
              options={[
                { value: '2048', label: '2048' },
                { value: '3072', label: '3072' },
                { value: '4096', label: '4096' },
              ]}
            />
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
  const [keyType, setKeyType] = useState<KeyType>('rsa')
  const [publicKey, setPublicKey] = useState('')
  const [privateKey, setPrivateKey] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deriving, setDeriving] = useState(false)
  const [deriveError, setDeriveError] = useState<string | null>(null)

  const handleDerive = async () => {
    setDeriveError(null)
    setDeriving(true)
    try {
      const pub = await derivePublicKeyFromPem(privateKey.trim(), passphrase || undefined)
      setPublicKey(pub)
      const detected = detectKeyType(pub)
      if (detected) setKeyType(detected)
    } catch (err) {
      setDeriveError(String(err))
    } finally {
      setDeriving(false)
    }
  }

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
          <Select
            value={keyType}
            onChange={(v) => setKeyType(v as KeyType)}
            ariaLabel={t('keys.keyType')}
            options={[
              { value: 'rsa', label: 'RSA' },
              { value: 'ed25519', label: 'Ed25519' },
              { value: 'ecdsa', label: 'ECDSA' },
              { value: 'dsa', label: 'DSA' },
            ]}
          />
          <p className="text-xs text-muted-foreground mt-1">{t('keys.importTypeNote')}</p>
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.publicKeyLabel')}</label>
          <textarea
            value={publicKey}
            onChange={(e) => setPublicKey(e.target.value)}
            className={`${inputClass} font-mono h-20 resize-none`}
            placeholder="ssh-ed25519 AAAA... user@host"
          />
          <p className="text-xs text-muted-foreground mt-1">{t('keys.publicKeyOptHint')}</p>
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
          <button
            type="button"
            onClick={handleDerive}
            disabled={deriving || !privateKey.trim()}
            className="mt-2 flex items-center gap-2 px-3 py-1.5 border border-phosphor/40 text-phosphor hover:bg-primary hover:text-primary-foreground hover:border-transparent transition-colors text-xs disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-phosphor"
          >
            <KeyRound className="h-3.5 w-3.5" />
            {deriving ? t('keys.deriving') : t('keys.derivePub')}
          </button>
          {deriveError && <p className="text-sm text-destructive break-all mt-1">{deriveError}</p>}
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
            disabled={importKey.isPending || !name.trim() || (!publicKey.trim() && !privateKey.trim())}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {importKey.isPending ? t('keys.importing') : t('keys.import')}
          </button>
        </div>
      </form>
    </ModalShell>
  )
}

function EditKeyDialog({ keyData, onClose }: { keyData: SshKey; onClose: () => void }) {
  const { t } = useT()
  const updateKey = useUpdateSshKey()
  const qc = useQueryClient()
  // Re-encrypt the existing key file with a new passphrase (separate action).
  const [curPass, setCurPass] = useState('')
  const [newPass, setNewPass] = useState('')
  const [ppBusy, setPpBusy] = useState(false)
  const [ppMsg, setPpMsg] = useState<{ ok: boolean; text: string } | null>(null)
  const [name, setName] = useState(keyData.name)
  const [keyType, setKeyType] = useState<KeyType>(keyData.keyType)
  const [publicKey, setPublicKey] = useState(keyData.publicKey)
  const [privateKey, setPrivateKey] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const [loadError, setLoadError] = useState<string | null>(null)
  const [deriving, setDeriving] = useState(false)
  const [deriveError, setDeriveError] = useState<string | null>(null)

  const handleDerive = async () => {
    setDeriveError(null)
    setDeriving(true)
    try {
      const pub = await derivePublicKeyFromPem(privateKey.trim(), passphrase || undefined)
      setPublicKey(pub)
      const detected = detectKeyType(pub)
      if (detected) setKeyType(detected)
    } catch (err) {
      setDeriveError(String(err))
    } finally {
      setDeriving(false)
    }
  }

  const handleLoadFile = async () => {
    setLoadError(null)
    let defaultPath: string | undefined
    try {
      defaultPath = `${await homeDir()}/.ssh`
    } catch {
      defaultPath = undefined
    }
    const path = await open({ multiple: false, directory: false, title: t('keys.dialogPickTitle'), defaultPath })
    if (typeof path !== 'string') return
    try {
      const loaded = await loadKeyFile(path)
      if (loaded.publicKey) {
        setPublicKey(loaded.publicKey)
        const detected = detectKeyType(loaded.publicKey)
        if (detected) setKeyType(detected)
      }
      if (loaded.privateKey) setPrivateKey(loaded.privateKey)
    } catch (err) {
      setLoadError(t('keys.loadError', { err: String(err) }))
    }
  }

  const handleChangePassphrase = async () => {
    setPpMsg(null)
    setPpBusy(true)
    try {
      await changeKeyPassphrase(keyData.id, curPass || undefined, newPass || undefined)
      setCurPass('')
      setNewPass('')
      setPpMsg({ ok: true, text: t('keys.passphraseChanged') })
      qc.invalidateQueries({ queryKey: sshKeyKeys.all })
    } catch (err) {
      setPpMsg({ ok: false, text: String(err) })
    } finally {
      setPpBusy(false)
    }
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    updateKey.mutate(
      {
        id: keyData.id,
        name: name.trim(),
        publicKey: publicKey.trim(),
        keyType,
        pemData: privateKey.trim() || undefined,
        passphrase: passphrase || undefined,
      },
      { onSuccess: onClose }
    )
  }

  return (
    <ModalShell title={t('keys.editTitle')} onClose={onClose}>
      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.name')}</label>
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} className={inputClass} required autoFocus />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.keyType')}</label>
          <Select
            value={keyType}
            onChange={(v) => setKeyType(v as KeyType)}
            ariaLabel={t('keys.keyType')}
            options={[
              { value: 'rsa', label: 'RSA' },
              { value: 'ed25519', label: 'Ed25519' },
              { value: 'ecdsa', label: 'ECDSA' },
              { value: 'dsa', label: 'DSA' },
            ]}
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.publicKeyLabel')}</label>
          <textarea
            value={publicKey}
            onChange={(e) => setPublicKey(e.target.value)}
            className={`${inputClass} font-mono h-20 resize-none`}
            placeholder="ssh-ed25519 AAAA... user@host"
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('keys.replacePrivateOpt')}</label>
          <button
            type="button"
            onClick={handleLoadFile}
            className="w-full flex items-center justify-center gap-2 px-3 py-2 border border-phosphor/40 text-phosphor hover:bg-primary hover:text-primary-foreground hover:border-transparent transition-colors text-sm mb-2"
          >
            <FolderOpen className="h-4 w-4" />
            {t('keys.loadFromFile')}
          </button>
          {loadError && <p className="text-sm text-destructive break-all mb-2">{loadError}</p>}
          <textarea
            value={privateKey}
            onChange={(e) => setPrivateKey(e.target.value)}
            className={`${inputClass} font-mono h-24 resize-none`}
            placeholder={t('keys.replacePrivatePlaceholder')}
          />
          <p className="text-xs text-muted-foreground mt-1">{t('keys.replacePrivateHint')}</p>
          <button
            type="button"
            onClick={handleDerive}
            disabled={deriving || !privateKey.trim()}
            className="mt-2 flex items-center gap-2 px-3 py-1.5 border border-phosphor/40 text-phosphor hover:bg-primary hover:text-primary-foreground hover:border-transparent transition-colors text-xs disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-phosphor"
          >
            <KeyRound className="h-3.5 w-3.5" />
            {deriving ? t('keys.deriving') : t('keys.derivePub')}
          </button>
          {deriveError && <p className="text-sm text-destructive break-all mt-1">{deriveError}</p>}
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

        {/* Re-encrypt the existing private key file (real ssh-keygen -p). */}
        <div className="border border-border rounded-md p-3 space-y-2">
          <p className="text-sm font-medium">{t('keys.changePassphrase')}</p>
          {keyData.passphraseProtected && (
            <input
              type="password"
              value={curPass}
              onChange={(e) => setCurPass(e.target.value)}
              className={inputClass}
              placeholder={t('keys.currentPassphrase')}
            />
          )}
          <input
            type="password"
            value={newPass}
            onChange={(e) => setNewPass(e.target.value)}
            className={inputClass}
            placeholder={t('keys.newPassphrase')}
          />
          <button
            type="button"
            onClick={handleChangePassphrase}
            disabled={ppBusy}
            className="px-3 py-1.5 border border-phosphor/40 text-phosphor hover:bg-primary hover:text-primary-foreground hover:border-transparent transition-colors text-xs disabled:opacity-40"
          >
            {ppBusy ? t('keys.changing') : t('keys.changePassphraseBtn')}
          </button>
          {ppMsg && (
            <p className={`text-xs break-all ${ppMsg.ok ? 'text-phosphor' : 'text-destructive'}`}>{ppMsg.text}</p>
          )}
        </div>

        {updateKey.isError && <p className="text-sm text-destructive break-all">{String(updateKey.error)}</p>}

        <div className="flex justify-end gap-2 pt-2">
          <button type="button" onClick={onClose} className="px-4 py-2 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80">
            {t('common.cancel')}
          </button>
          <button
            type="submit"
            disabled={updateKey.isPending || !name.trim()}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
          >
            {updateKey.isPending ? t('common.saving') : t('common.save')}
          </button>
        </div>
      </form>
    </ModalShell>
  )
}

function KeyCard({ keyData, onEdit }: { keyData: SshKey; onEdit: () => void }) {
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
          onClick={onEdit}
          title={t('keys.edit')}
          className="p-1.5 rounded-md hover:bg-muted text-muted-foreground hover:text-phosphor"
        >
          <Pencil className="h-4 w-4" />
        </button>
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
  const [editKey, setEditKey] = useState<SshKey | null>(null)
  const { data: keys = [], isLoading } = useSshKeys()

  return (
    <div className="p-6 max-w-5xl">
      <div className="flex items-end justify-between mb-6 crt-in">
        <div>
          <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
            {t('keys.subtitle', { n: keys.length })}
          </p>
          <h1 className="font-display text-5xl leading-none text-foreground">
            SSH KEYS
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
            <KeyCard key={key.id} keyData={key} onEdit={() => setEditKey(key)} />
          ))}
        </div>
      )}

      {showCreateDialog && <CreateKeyDialog onClose={() => setShowCreateDialog(false)} />}
      {showImportDialog && <ImportKeyDialog onClose={() => setShowImportDialog(false)} />}
      {editKey && <EditKeyDialog keyData={editKey} onClose={() => setEditKey(null)} />}
    </div>
  )
}
