import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { openFileDialog, saveFileDialog } from '@/lib/bridge'
import { syncServersToConfig, syncConfigToServers, exportData, importData } from '@/lib/commands'
import { useEffect } from 'react'
import { serverKeys, useServers } from '@/hooks/useServers'
import { sshKeyKeys, useSshKeys } from '@/hooks/useKeys'
import { useT } from '@/contexts/LanguageContext'
import { useShortcuts } from '@/contexts/ShortcutsContext'
import { useTheme } from '@/contexts/ThemeContext'
import { ACCENT_PRESETS, type BgPreset } from '@/lib/theme'
import { SHORTCUT_ACTIONS, comboFromEvent, formatCombo, isModifierOnly, type ShortcutAction } from '@/lib/shortcuts'
import { LANGS, type Lang } from '@/i18n'
import { Select } from '@/components/Select'
import { START_ROUTES, loadStartRoute, saveStartRoute } from '@/lib/startup'
import { ArrowRight, ArrowLeft, Upload, Download, KeyRound, X } from 'lucide-react'

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
  const { theme, setTheme } = useTheme()
  const [capturing, setCapturing] = useState<ShortcutAction | null>(null)
  const [startRoute, setStartRoute] = useState(loadStartRoute)
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

  // Export selection modal
  const { data: serverList = [] } = useServers()
  const { data: keyList = [] } = useSshKeys()
  const [exportSel, setExportSel] = useState<{ encrypted: boolean } | null>(null)
  const [selServers, setSelServers] = useState<Set<number>>(new Set())
  const [selKeys, setSelKeys] = useState<Set<number>>(new Set())
  const [inclShortcuts, setInclShortcuts] = useState(true)

  const showImportResult = (s: import('@/lib/commands').ImportSummary) => {
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

  // Open the selection modal; default to everything selected.
  const openExportSelect = (encrypted: boolean) => {
    setMessage(null)
    setSelServers(new Set(serverList.map((s) => s.id)))
    setSelKeys(new Set(keyList.map((k) => k.id)))
    setInclShortcuts(true)
    setExportSel({ encrypted })
  }

  const toggle = (set: Set<number>, id: number) => {
    const next = new Set(set)
    next.has(id) ? next.delete(id) : next.add(id)
    return next
  }

  // After selection: pick a path, then export (encrypted goes via the passphrase modal).
  const confirmExportSelect = async () => {
    if (!exportSel) return
    const { encrypted } = exportSel
    setExportSel(null)
    const path = await saveFileDialog({
      title: t('settings.exportDialogTitle'),
      defaultPath: encrypted ? 'sshub-export.enc' : 'sshub-export.json',
      filters: [{ name: 'sshub', extensions: [encrypted ? 'enc' : 'json'] }],
    })
    if (!path) return
    if (encrypted) {
      setPph('')
      setPphModal({ mode: 'export', path })
      return
    }
    try {
      await exportData(path, {
        shortcuts: inclShortcuts ? shortcuts : undefined,
        serverIds: [...selServers],
        keyIds: [...selKeys],
      })
      setMessage(t('settings.exportDone', { path }))
    } catch (error) {
      setMessage(t('settings.exportFail', { err: String(error) }))
    }
  }

  const handleImport = async () => {
    setMessage(null)
    const path = await openFileDialog({
      title: t('settings.importDialogTitle'),
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
        await exportData(path, {
          passphrase: pph,
          shortcuts: inclShortcuts ? shortcuts : undefined,
          serverIds: [...selServers],
          keyIds: [...selKeys],
        })
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
    <div className="p-6 max-w-5xl">
      <div className="mb-6 crt-in">
        <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
          ~/settings
        </p>
        <h1 className="font-display text-5xl leading-none text-foreground">
          SETTINGS
        </h1>
      </div>

      {/* 1 column when narrow; 2 from lg (≥1024px). CSS multi-column packs cards
          by height (no ragged grid gaps); break-inside-avoid keeps each intact.
          Order is column-major: top→bottom of the left column, then the right. */}
      <div className="columns-1 lg:columns-2 gap-6 [&>div]:mb-6 [&>div]:break-inside-avoid">
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
                onClick={() => openExportSelect(false)}
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
                onClick={() => openExportSelect(true)}
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

        <div className="bg-card border border-border p-5 crt-in" style={{ animationDelay: '140ms' }}>
          <SectionTitle>General</SectionTitle>
          <div className="flex items-center justify-between p-3 bg-muted/60 border border-border gap-4">
            <div>
              <h3 className="text-sm font-semibold">{t('settings.startMenu')}</h3>
              <p className="text-xs text-muted-foreground">{t('settings.startMenuDesc')}</p>
            </div>
            <Select
              value={startRoute}
              onChange={(v) => {
                setStartRoute(v)
                saveStartRoute(v)
              }}
              ariaLabel={t('settings.startMenu')}
              className="w-44 shrink-0"
              options={START_ROUTES.map((r) => ({ value: r.route, label: t(r.labelKey) }))}
            />
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
          <SectionTitle>Appearance</SectionTitle>
          <div className="space-y-2">
            {/* Accent color */}
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <h3 className="text-sm font-semibold">{t('settings.accent')}</h3>
              <div className="flex items-center gap-2">
                {ACCENT_PRESETS.map((p) => (
                  <button
                    key={p.name}
                    onClick={() => setTheme({ accent: p.value })}
                    title={p.name}
                    className={`w-5 h-5 rounded-full border-2 ${
                      theme.accent.toLowerCase() === p.value.toLowerCase()
                        ? 'border-foreground'
                        : 'border-transparent'
                    }`}
                    style={{ background: p.value }}
                  />
                ))}
                <input
                  type="color"
                  value={theme.accent}
                  onChange={(e) => setTheme({ accent: e.target.value })}
                  title={t('settings.custom')}
                  className="w-6 h-6 bg-transparent cursor-pointer"
                />
              </div>
            </div>

            {/* Background tone */}
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <h3 className="text-sm font-semibold">{t('settings.bgTone')}</h3>
              <div className="flex">
                {(['green', 'neutral', 'warm', 'black'] as BgPreset[]).map((b) => (
                  <button
                    key={b}
                    onClick={() => setTheme({ bg: b })}
                    className={`px-2.5 py-1 text-[10px] uppercase tracking-wider border transition-colors ${
                      theme.bg === b
                        ? 'border-phosphor text-phosphor bg-accent'
                        : 'border-border text-muted-foreground hover:text-foreground'
                    }`}
                  >
                    {b}
                  </button>
                ))}
              </div>
            </div>

            {/* Terminal colors */}
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <h3 className="text-sm font-semibold">{t('settings.termColors')}</h3>
              <div className="flex items-center gap-4 text-xs text-muted-foreground">
                <label className="flex items-center gap-1.5">
                  {t('settings.termFg')}
                  <input
                    type="color"
                    value={theme.termFg}
                    onChange={(e) => setTheme({ termFg: e.target.value })}
                    className="w-6 h-6 bg-transparent cursor-pointer"
                  />
                </label>
                <label className="flex items-center gap-1.5">
                  {t('settings.termBg')}
                  <input
                    type="color"
                    value={theme.termBg}
                    onChange={(e) => setTheme({ termBg: e.target.value })}
                    className="w-6 h-6 bg-transparent cursor-pointer"
                  />
                </label>
              </div>
            </div>

            {/* Terminal font size */}
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <h3 className="text-sm font-semibold">{t('settings.termFontSize')}</h3>
              <div className="flex items-center gap-2">
                <input
                  type="range"
                  min={10}
                  max={24}
                  value={theme.termFontSize}
                  onChange={(e) => setTheme({ termFontSize: Number(e.target.value) })}
                  className="accent-[var(--phosphor)]"
                />
                <span className="text-xs text-muted-foreground tabular-nums w-9 text-right">
                  {theme.termFontSize}px
                </span>
              </div>
            </div>

            {/* UI translucency */}
            <div className="flex items-center justify-between p-3 bg-muted/60 border border-border">
              <h3 className="text-sm font-semibold">{t('settings.uiOpacity')}</h3>
              <div className="flex items-center gap-2">
                <input
                  type="range"
                  min={0}
                  max={40}
                  value={theme.opacity}
                  onChange={(e) => setTheme({ opacity: Number(e.target.value) })}
                  className="accent-[var(--phosphor)]"
                />
                <span className="text-xs text-muted-foreground tabular-nums w-9 text-right">
                  {theme.opacity}%
                </span>
              </div>
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
              <span className="text-phosphor/70 mr-2">ver</span>
              {__APP_VERSION__}
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

      {exportSel && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-card border border-border p-5 w-full max-w-md max-h-[85vh] flex flex-col">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-lg font-semibold">{t('settings.exportSelectTitle')}</h2>
              <button onClick={() => setExportSel(null)} className="p-1 hover:bg-muted">
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="flex items-center justify-end mb-2">
              <button
                onClick={() => {
                  const allSel = selServers.size === serverList.length && selKeys.size === keyList.length
                  setSelServers(allSel ? new Set() : new Set(serverList.map((s) => s.id)))
                  setSelKeys(allSel ? new Set() : new Set(keyList.map((k) => k.id)))
                }}
                className="text-xs text-muted-foreground hover:text-phosphor"
              >
                {t('settings.selectAll')}
              </button>
            </div>

            <div className="flex-1 overflow-y-auto border border-border divide-y divide-border">
              <div className="px-3 py-1 text-[9px] tracking-[0.25em] uppercase text-muted-foreground/70 bg-muted/40">
                {t('nav.servers')} ({selServers.size}/{serverList.length})
              </div>
              {serverList.length === 0 ? (
                <p className="px-3 py-2 text-xs text-muted-foreground/70">—</p>
              ) : (
                serverList.map((s) => (
                  <label key={s.id} className="flex items-center gap-2 px-3 py-1.5 text-xs cursor-pointer hover:bg-accent/40">
                    <input
                      type="checkbox"
                      checked={selServers.has(s.id)}
                      onChange={() => setSelServers((prev) => toggle(prev, s.id))}
                      className="accent-[var(--phosphor)]"
                    />
                    <span className="truncate">
                      {s.name}{' '}
                      <span className="text-muted-foreground/70">
                        {s.username}@{s.host}
                      </span>
                    </span>
                  </label>
                ))
              )}
              <div className="px-3 py-1 text-[9px] tracking-[0.25em] uppercase text-muted-foreground/70 bg-muted/40">
                SSH Keys ({selKeys.size}/{keyList.length})
              </div>
              {keyList.length === 0 ? (
                <p className="px-3 py-2 text-xs text-muted-foreground/70">—</p>
              ) : (
                keyList.map((k) => (
                  <label key={k.id} className="flex items-center gap-2 px-3 py-1.5 text-xs cursor-pointer hover:bg-accent/40">
                    <input
                      type="checkbox"
                      checked={selKeys.has(k.id)}
                      onChange={() => setSelKeys((prev) => toggle(prev, k.id))}
                      className="accent-[var(--phosphor)]"
                    />
                    <span className="truncate">
                      {k.name} <span className="text-muted-foreground/70">({k.keyType})</span>
                    </span>
                  </label>
                ))
              )}
            </div>

            <label className="flex items-center gap-2 mt-3 text-xs cursor-pointer">
              <input
                type="checkbox"
                checked={inclShortcuts}
                onChange={(e) => setInclShortcuts(e.target.checked)}
                className="accent-[var(--phosphor)]"
              />
              {t('settings.includeShortcuts')}
            </label>

            <div className="flex justify-end gap-2 mt-4">
              <button
                onClick={() => setExportSel(null)}
                className="px-4 py-2 border border-border text-muted-foreground hover:text-foreground transition-colors text-sm"
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={confirmExportSelect}
                disabled={selServers.size === 0 && selKeys.size === 0 && !inclShortcuts}
                className="px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium disabled:opacity-50"
              >
                {t('common.export')}
              </button>
            </div>
          </div>
        </div>
      )}

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
