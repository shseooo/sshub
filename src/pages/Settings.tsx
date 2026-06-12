import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { open, save } from '@tauri-apps/plugin-dialog'
import { syncServersToConfig, syncConfigToServers, exportData, importData } from '@/lib/tauriCommands'
import { useEffect } from 'react'
import { serverKeys } from '@/hooks/useServers'
import { sshKeyKeys } from '@/hooks/useKeys'
import { useT } from '@/contexts/LanguageContext'
import { useShortcuts } from '@/contexts/ShortcutsContext'
import { SHORTCUT_ACTIONS, comboFromEvent, formatCombo, isModifierOnly, type ShortcutAction } from '@/lib/shortcuts'
import { LANGS, type Lang } from '@/i18n'
import { ArrowRight, ArrowLeft, Upload, Download, KeyRound, X } from 'lucide-react'

type Phosphor = 'green' | 'amber'

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-4 flex items-center gap-2">
      {children}
      <span className="flex-1 border-t border-border" />
    </h2>
  )
}

export default function Settings() {
  const { t, lang, setLang } = useT()
  const { shortcuts, setShortcut, replaceShortcuts } = useShortcuts()
  const [capturing, setCapturing] = useState<ShortcutAction | null>(null)
  const queryClient = useQueryClient()
  const [message, setMessage] = useState<string | null>(null)

  // While rebinding, capture the next real key combo (Esc cancels).
  useEffect(() => {
    if (!capturing) return
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.code === 'Escape') {
        setCapturing(null)
        return
      }
      if (isModifierOnly(e.code)) return
      setShortcut(capturing, comboFromEvent(e))
      setCapturing(null)
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [capturing, setShortcut])
  const [phosphor, setPhosphor] = useState<Phosphor>(() =>
    document.documentElement.classList.contains('amber') ? 'amber' : 'green'
  )

  const applyPhosphor = (next: Phosphor) => {
    document.documentElement.classList.toggle('amber', next === 'amber')
    localStorage.setItem('phosphor', next)
    setPhosphor(next)
  }

  const syncTo = useMutation({
    mutationFn: syncServersToConfig,
    onSuccess: () => setMessage(t('settings.syncToDone')),
    onError: (error) => setMessage(t('settings.syncToFail', { err: String(error) })),
  })

  const syncFrom = useMutation({
    mutationFn: syncConfigToServers,
    onSuccess: (imported) => {
      setMessage(
        imported.length > 0
          ? t('settings.importFromConfigDone', {
              n: imported.length,
              names: imported.map((s) => s.name).join(', '),
            })
          : t('settings.importFromConfigNone')
      )
      queryClient.invalidateQueries({ queryKey: serverKeys.all })
    },
    onError: (error) => setMessage(t('settings.importFail', { err: String(error) })),
  })

  // Passphrase modal state: 'export' sets one, 'import' enters one to decrypt.
  const [pphModal, setPphModal] = useState<{ mode: 'export' | 'import'; path: string } | null>(null)
  const [pph, setPph] = useState('')

  const showImportResult = (s: import('@/lib/tauriCommands').ImportSummary) => {
    setMessage(
      t('settings.importDone', {
        sa: s.serversAdded,
        ss: s.serversSkipped,
        ka: s.keysAdded,
        ks: s.keysSkipped,
      })
    )
    if (s.shortcuts) replaceShortcuts(s.shortcuts)
    queryClient.invalidateQueries({ queryKey: serverKeys.all })
    queryClient.invalidateQueries({ queryKey: sshKeyKeys.all })
  }

  // Plain export — no secrets, safe to sync anywhere.
  const handleExport = async () => {
    setMessage(null)
    const path = await save({
      title: t('settings.exportDialogTitle'),
      defaultPath: 'sshub-export.json',
      filters: [{ name: 'JSON', extensions: ['json'] }],
    })
    if (!path) return
    try {
      await exportData(path, undefined, shortcuts)
      setMessage(t('settings.exportDone', { path }))
    } catch (error) {
      setMessage(t('settings.exportFail', { err: String(error) }))
    }
  }

  // Encrypted export with private keys — pick a path, then ask for a passphrase.
  const handleSecureExport = async () => {
    setMessage(null)
    const path = await save({
      title: t('settings.exportDialogTitle'),
      defaultPath: 'sshub-export.enc',
      filters: [{ name: 'sshub', extensions: ['enc'] }],
    })
    if (!path) return
    setPph('')
    setPphModal({ mode: 'export', path })
  }

  const handleImport = async () => {
    setMessage(null)
    const path = await open({
      title: t('settings.importDialogTitle'),
      multiple: false,
      directory: false,
      filters: [{ name: 'sshub', extensions: ['json', 'enc'] }],
    })
    if (typeof path !== 'string') return
    try {
      showImportResult(await importData(path))
    } catch (error) {
      if (String(error).includes('ENCRYPTED')) {
        // File is encrypted — ask for the passphrase and retry.
        setPph('')
        setPphModal({ mode: 'import', path })
      } else {
        setMessage(t('settings.importFail', { err: String(error) }))
      }
    }
  }

  const submitPassphrase = async () => {
    if (!pphModal || !pph) return
    const { mode, path } = pphModal
    setPphModal(null)
    try {
      if (mode === 'export') {
        await exportData(path, pph, shortcuts)
        setMessage(t('settings.exportEncryptedDone', { path }))
      } else {
        showImportResult(await importData(path, pph))
      }
    } catch (error) {
      setMessage(
        mode === 'export'
          ? t('settings.exportFail', { err: String(error) })
          : t('settings.importFail', { err: String(error) })
      )
    } finally {
      setPph('')
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <div className="mb-6 crt-in">
        <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
          ~/settings
        </p>
        <h1 className="font-display text-5xl leading-none text-foreground">
          SETTINGS
        </h1>
      </div>

      <div className="space-y-6">
        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '50ms' }}>
          <SectionTitle>SSH Config Sync</SectionTitle>
          <p className="text-xs text-muted-foreground mb-4">{t('settings.syncDesc')}</p>

          <div className="space-y-2">
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <div>
                <h3 className="text-sm font-semibold">{t('settings.toConfigTitle')}</h3>
                <p className="text-xs text-muted-foreground">{t('settings.toConfigDesc')}</p>
              </div>
              <button
                onClick={() => syncTo.mutate()}
                disabled={syncTo.isPending}
                className="flex items-center gap-2 px-3 py-1.5 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-xs font-medium disabled:opacity-50"
              >
                <ArrowRight className="h-3 w-3" />
                {syncTo.isPending ? t('settings.syncing') : t('settings.sync')}
              </button>
            </div>

            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <div>
                <h3 className="text-sm font-semibold">{t('settings.fromConfigTitle')}</h3>
                <p className="text-xs text-muted-foreground">{t('settings.fromConfigDesc')}</p>
              </div>
              <button
                onClick={() => syncFrom.mutate()}
                disabled={syncFrom.isPending}
                className="flex items-center gap-2 px-3 py-1.5 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-xs font-medium disabled:opacity-50"
              >
                <ArrowLeft className="h-3 w-3" />
                {syncFrom.isPending ? t('common.importing') : t('common.import')}
              </button>
            </div>
          </div>

          {message && (
            <div className="mt-4 p-3 border border-phosphor/30 bg-accent text-xs break-all">
              <span className="text-phosphor mr-1.5">»</span>
              {message}
            </div>
          )}
        </div>

        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '100ms' }}>
          <SectionTitle>Backup / Sync</SectionTitle>
          <p className="text-xs text-muted-foreground mb-4">{t('settings.backupDesc')}</p>

          <div className="space-y-2">
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <div>
                <h3 className="text-sm font-semibold">{t('common.export')}</h3>
                <p className="text-xs text-muted-foreground">{t('settings.exportDesc')}</p>
              </div>
              <button
                onClick={handleExport}
                className="flex items-center gap-2 px-3 py-1.5 border border-border text-muted-foreground hover:text-foreground hover:border-muted-foreground transition-colors text-xs"
              >
                <Download className="h-3 w-3" />
                {t('common.export')}
              </button>
            </div>

            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <div>
                <h3 className="text-sm font-semibold">{t('settings.exportWithKeys')}</h3>
                <p className="text-xs text-muted-foreground">{t('settings.exportWithKeysDesc')}</p>
              </div>
              <button
                onClick={handleSecureExport}
                className="flex items-center gap-2 px-3 py-1.5 border border-border text-muted-foreground hover:text-foreground hover:border-muted-foreground transition-colors text-xs whitespace-nowrap"
              >
                <KeyRound className="h-3 w-3" />
                {t('settings.exportWithKeys')}
              </button>
            </div>

            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <div>
                <h3 className="text-sm font-semibold">{t('common.import')}</h3>
                <p className="text-xs text-muted-foreground">{t('settings.importDesc')}</p>
              </div>
              <button
                onClick={handleImport}
                className="flex items-center gap-2 px-3 py-1.5 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-xs font-medium"
              >
                <Upload className="h-3 w-3" />
                {t('common.import')}
              </button>
            </div>
          </div>
        </div>

        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '150ms' }}>
          <SectionTitle>Language</SectionTitle>
          <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
            <div>
              <h3 className="text-sm font-semibold">{t('settings.language')}</h3>
              <p className="text-xs text-muted-foreground">{t('settings.languageDesc')}</p>
            </div>
            <div className="flex">
              {LANGS.map((l) => (
                <button
                  key={l.code}
                  onClick={() => setLang(l.code as Lang)}
                  className={`px-3 py-1.5 text-xs border transition-colors ${
                    lang === l.code
                      ? 'border-phosphor text-phosphor bg-accent'
                      : 'border-border text-muted-foreground hover:text-foreground'
                  }`}
                >
                  {l.label}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '200ms' }}>
          <SectionTitle>Phosphor</SectionTitle>
          <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
            <div>
              <h3 className="text-sm font-semibold">{t('settings.phosphorTitle')}</h3>
              <p className="text-xs text-muted-foreground">{t('settings.phosphorDesc')}</p>
            </div>
            <div className="flex">
              {(['green', 'amber'] as const).map((p) => (
                <button
                  key={p}
                  onClick={() => applyPhosphor(p)}
                  className={`flex items-center gap-2 px-3 py-1.5 text-xs uppercase tracking-wider border transition-colors ${
                    phosphor === p
                      ? 'border-phosphor text-phosphor bg-accent'
                      : 'border-border text-muted-foreground hover:text-foreground'
                  }`}
                >
                  <span
                    className="inline-block w-2.5 h-2.5 rounded-full"
                    style={{ background: p === 'green' ? '#3dff88' : '#ffb347' }}
                  />
                  {p}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '250ms' }}>
          <SectionTitle>Shortcuts</SectionTitle>
          <p className="text-xs text-muted-foreground mb-4">{t('settings.shortcutsDesc')}</p>
          <div className="space-y-2">
            {SHORTCUT_ACTIONS.map(({ action, labelKey }) => (
              <div
                key={action}
                className="flex items-center justify-between p-3 bg-muted/60 border border-border"
              >
                <h3 className="text-sm font-semibold">{t(labelKey)}</h3>
                <button
                  onClick={() => setCapturing(action)}
                  className={`min-w-[72px] px-3 py-1.5 text-xs border transition-colors ${
                    capturing === action
                      ? 'border-phosphor text-phosphor bg-accent animate-pulse'
                      : 'border-border text-muted-foreground hover:text-phosphor hover:border-phosphor/60'
                  }`}
                >
                  {capturing === action ? t('settings.pressKeys') : formatCombo(shortcuts[action])}
                </button>
              </div>
            ))}
          </div>
        </div>

        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '300ms' }}>
          <SectionTitle>System Info</SectionTitle>
          <div className="space-y-1.5 text-xs text-muted-foreground">
            <p>
              <span className="text-phosphor/70 mr-2">ver</span>0.1.1
            </p>
            <p>
              <span className="text-phosphor/70 mr-2">data</span>
              ~/Library/Application Support/sshub.json
            </p>
            <p>
              <span className="text-phosphor/70 mr-2">keys</span>
              ~/Library/Application Support/ssh_keys/
            </p>
          </div>
        </div>
      </div>

      {pphModal && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card border border-border p-6 w-full max-w-sm">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-lg font-semibold">{t('settings.passphrase')}</h2>
              <button onClick={() => setPphModal(null)} className="p-1 hover:bg-muted">
                <X className="h-4 w-4" />
              </button>
            </div>
            <p className="text-xs text-muted-foreground mb-3">
              {pphModal.mode === 'export'
                ? t('settings.passphraseExportHint')
                : t('settings.passphraseImportHint')}
            </p>
            <input
              type="password"
              value={pph}
              onChange={(e) => setPph(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submitPassphrase()}
              className="w-full px-3 py-2 bg-background border border-border focus:outline-hidden focus:border-phosphor/60 focus:ring-1 focus:ring-phosphor/40 mb-4"
              placeholder={t('settings.passphrase')}
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setPphModal(null)}
                className="px-4 py-2 border border-border text-muted-foreground hover:text-foreground transition-colors text-sm"
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={submitPassphrase}
                disabled={!pph}
                className="px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium disabled:opacity-50"
              >
                {pphModal.mode === 'export' ? t('common.export') : t('settings.decrypt')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
