import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { open, save } from '@tauri-apps/plugin-dialog'
import { syncServersToConfig, syncConfigToServers, exportData, importData } from '@/lib/tauriCommands'
import { serverKeys } from '@/hooks/useServers'
import { sshKeyKeys } from '@/hooks/useKeys'
import { useT } from '@/contexts/LanguageContext'
import { LANGS, type Lang } from '@/i18n'
import { ArrowRight, ArrowLeft, Upload, Download } from 'lucide-react'

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
  const queryClient = useQueryClient()
  const [message, setMessage] = useState<string | null>(null)
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

  const handleExport = async () => {
    setMessage(null)
    const path = await save({
      title: t('settings.exportDialogTitle'),
      defaultPath: 'sshub-export.json',
      filters: [{ name: 'JSON', extensions: ['json'] }],
    })
    if (!path) return
    try {
      await exportData(path)
      setMessage(t('settings.exportDone', { path }))
    } catch (error) {
      setMessage(t('settings.exportFail', { err: String(error) }))
    }
  }

  const handleImport = async () => {
    setMessage(null)
    const path = await open({
      title: t('settings.importDialogTitle'),
      multiple: false,
      directory: false,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    })
    if (typeof path !== 'string') return
    try {
      const s = await importData(path)
      setMessage(
        t('settings.importDone', {
          sa: s.serversAdded,
          ss: s.serversSkipped,
          ka: s.keysAdded,
          ks: s.keysSkipped,
        })
      )
      queryClient.invalidateQueries({ queryKey: serverKeys.all })
      queryClient.invalidateQueries({ queryKey: sshKeyKeys.all })
    } catch (error) {
      setMessage(t('settings.importFail', { err: String(error) }))
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <div className="mb-6 crt-in">
        <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
          ~/settings
        </p>
        <h1 className="font-display text-5xl leading-none text-foreground">
          SETTINGS<span className="text-phosphor animate-blink">▮</span>
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
          <SectionTitle>System Info</SectionTitle>
          <div className="space-y-1.5 text-xs text-muted-foreground">
            <p>
              <span className="text-phosphor/70 mr-2">ver</span>0.1.0
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
    </div>
  )
}
