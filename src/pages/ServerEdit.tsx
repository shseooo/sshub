import { useState, useEffect } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { z } from 'zod'
import { useSaveServer, useServer } from '@/hooks/useServers'
import { useSshKeys } from '@/hooks/useKeys'
import { useT } from '@/contexts/LanguageContext'
import { Select } from '@/components/Select'
import type { AuthType, CreateServerDto } from '@/types/server'

// Messages are i18n keys; translated when shown.
const serverSchema = z.object({
  name: z.string().trim().min(1, 'edit.errName'),
  host: z.string().trim().min(1, 'edit.errHost'),
  port: z.number().int('edit.errPortInt').min(1, 'edit.errPortMin').max(65535, 'edit.errPortMax'),
  username: z.string().trim().min(1, 'edit.errUser'),
})

/** DB의 tags(JSON 문자열 배열)를 쉼표 구분 문자열로 */
function tagsToInput(tags: string | null): string {
  if (!tags) return ''
  try {
    const parsed = JSON.parse(tags)
    return Array.isArray(parsed) ? parsed.join(', ') : tags
  } catch {
    return tags
  }
}

function inputToTags(input: string): string | undefined {
  const items = input.split(',').map((t) => t.trim()).filter(Boolean)
  return items.length > 0 ? JSON.stringify(items) : undefined
}

const fieldClass = 'w-full px-3 py-2 bg-background border border-border focus:outline-hidden focus:border-phosphor/60 focus:ring-1 focus:ring-phosphor/40'

export default function ServerEdit() {
  const { id } = useParams<{ id?: string }>()
  const navigate = useNavigate()
  const { t } = useT()
  const isEdit = !!id

  const { data: server, isLoading } = useServer(id ? Number(id) : undefined)
  const { data: keys = [] } = useSshKeys()
  const saveServer = useSaveServer()

  const [name, setName] = useState('')
  const [host, setHost] = useState('')
  const [port, setPort] = useState(22)
  const [username, setUsername] = useState('')
  const [authType, setAuthType] = useState<AuthType>('key')
  const [keyId, setKeyId] = useState<number | ''>('')
  const [pemData, setPemData] = useState('')
  const [proxyJump, setProxyJump] = useState('')
  const [groupName, setGroupName] = useState('')
  const [tags, setTags] = useState('')
  const [notes, setNotes] = useState('')
  const [formError, setFormError] = useState<string | null>(null)

  useEffect(() => {
    if (server) {
      setName(server.name)
      setHost(server.host)
      setPort(server.port)
      setUsername(server.username)
      setAuthType(server.authType)
      setKeyId(server.keyId ?? '')
      setPemData(server.pemData ?? '')
      setProxyJump(server.proxyJump ?? '')
      setGroupName(server.groupName ?? '')
      setTags(tagsToInput(server.tags))
      setNotes(server.notes ?? '')
    }
  }, [server])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    setFormError(null)

    const parsed = serverSchema.safeParse({ name, host, port, username })
    if (!parsed.success) {
      setFormError(t(parsed.error.issues[0].message))
      return
    }

    const dto: CreateServerDto = {
      ...parsed.data,
      authType,
      keyId: authType === 'key' && keyId !== '' ? keyId : undefined,
      pemData: authType === 'pem' && pemData.trim() ? pemData.trim() : undefined,
      proxyJump: proxyJump.trim() || undefined,
      groupName: groupName.trim() || undefined,
      tags: inputToTags(tags),
      notes: notes.trim() || undefined,
    }

    saveServer.mutate(isEdit && id ? { ...dto, id: Number(id) } : dto, {
      onSuccess: () => navigate('/servers'),
      onError: (error) => setFormError(String(error)),
    })
  }

  if (isEdit && isLoading) {
    return <div className="p-6">{t('common.loading')}</div>
  }

  return (
    <div className="p-6 max-w-2xl">
      <div className="mb-6 crt-in">
        <p className="text-[10px] tracking-[0.25em] uppercase text-muted-foreground mb-1">
          {isEdit ? `~/servers/${id}/edit` : '~/servers/new'}
        </p>
        <h1 className="font-display text-5xl leading-none text-foreground">
          {isEdit ? 'EDIT SERVER' : 'NEW SERVER'}
        </h1>
      </div>

      <form onSubmit={handleSubmit} className="space-y-4 bg-card border border-border p-5 crt-in" style={{ animationDelay: '50ms' }}>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium mb-1">{t('edit.name')} *</label>
            <input type="text" value={name} onChange={(e) => setName(e.target.value)} className={fieldClass} required />
          </div>
          <div>
            <label className="block text-sm font-medium mb-1">{t('edit.group')}</label>
            <input type="text" value={groupName} onChange={(e) => setGroupName(e.target.value)} className={fieldClass} placeholder={t('edit.groupPlaceholder')} />
          </div>
        </div>

        <div className="grid grid-cols-3 gap-4">
          <div className="col-span-2">
            <label className="block text-sm font-medium mb-1">{t('edit.host')} *</label>
            <input type="text" value={host} onChange={(e) => setHost(e.target.value)} className={fieldClass} placeholder={t('edit.hostPlaceholder')} required />
          </div>
          <div>
            <label className="block text-sm font-medium mb-1">{t('edit.port')}</label>
            <input type="number" value={port} onChange={(e) => setPort(Number(e.target.value))} className={fieldClass} min={1} max={65535} />
          </div>
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('edit.user')} *</label>
          <input type="text" value={username} onChange={(e) => setUsername(e.target.value)} className={fieldClass} placeholder={t('edit.userPlaceholder')} required />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('edit.authType')}</label>
          <Select
            value={authType}
            onChange={(v) => setAuthType(v as AuthType)}
            ariaLabel={t('edit.authType')}
            options={[
              { value: 'key', label: t('edit.authKey') },
              { value: 'password', label: t('edit.authPassword') },
              { value: 'pem', label: t('edit.authPem') },
              { value: 'agent', label: t('edit.authAgent') },
            ]}
          />
        </div>

        {authType === 'key' && (
          <div>
            <label className="block text-sm font-medium mb-1">{t('edit.keySelect')}</label>
            <Select
              value={keyId === '' ? '' : String(keyId)}
              onChange={(v) => setKeyId(v === '' ? '' : Number(v))}
              ariaLabel={t('edit.keySelect')}
              options={[
                { value: '', label: t('edit.keyDefault') },
                ...keys.map((key) => ({ value: String(key.id), label: `${key.name} (${key.keyType})` })),
              ]}
            />
            <p className="text-xs text-muted-foreground mt-1">{t('edit.keyHint')}</p>
          </div>
        )}

        {authType === 'password' && (
          <p className="text-xs text-muted-foreground">{t('edit.passwordHint')}</p>
        )}

        {authType === 'pem' && (
          <div>
            <label className="block text-sm font-medium mb-1">{t('edit.pemLabel')}</label>
            <textarea
              value={pemData}
              onChange={(e) => setPemData(e.target.value)}
              className={`${fieldClass} font-mono h-32 resize-none`}
              placeholder={'-----BEGIN RSA PRIVATE KEY-----\n...'}
            />
            <p className="text-xs text-muted-foreground mt-1">
              {isEdit && !pemData.trim() ? t('edit.pemKeptHint') : t('edit.pemHint')}
            </p>
          </div>
        )}

        {authType === 'agent' && (
          <p className="text-xs text-muted-foreground">{t('edit.agentHint')}</p>
        )}

        <div>
          <label className="block text-sm font-medium mb-1">{t('edit.proxyJump')}</label>
          <input
            type="text"
            value={proxyJump}
            onChange={(e) => setProxyJump(e.target.value)}
            className={fieldClass}
            placeholder="user@bastion.example.com"
          />
          <p className="text-xs text-muted-foreground mt-1">{t('edit.proxyJumpHint')}</p>
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('edit.tags')}</label>
          <input type="text" value={tags} onChange={(e) => setTags(e.target.value)} className={fieldClass} placeholder={t('edit.tagsPlaceholder')} />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1">{t('edit.notes')}</label>
          <textarea value={notes} onChange={(e) => setNotes(e.target.value)} className={`${fieldClass} h-20 resize-none`} />
        </div>

        {formError && <p className="text-sm text-destructive">{formError}</p>}

        <div className="flex gap-3 pt-4">
          <button
            type="submit"
            disabled={saveServer.isPending}
            className="px-5 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors font-medium disabled:opacity-50"
          >
            {saveServer.isPending ? t('common.saving') : t('common.save')}
          </button>
          <button
            type="button"
            onClick={() => navigate('/servers')}
            className="px-5 py-2 border border-border text-muted-foreground hover:text-foreground hover:border-muted-foreground transition-colors"
          >
            {t('common.cancel')}
          </button>
        </div>
      </form>
    </div>
  )
}
