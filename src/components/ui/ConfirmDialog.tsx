import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react'
import { useT } from '@/contexts/LanguageContext'

// App-themed replacement for the browser's native confirm() dialog, which
// rendered in the OS default style and clashed with the CRT/phosphor UI. Call
// sites `await confirm({...})` and get a boolean, matching the old control flow.

interface ConfirmOptions {
  title?: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  /** Style the confirm button as destructive (delete/close). */
  danger?: boolean
}

type ConfirmFn = (opts: ConfirmOptions) => Promise<boolean>

const ConfirmContext = createContext<ConfirmFn | null>(null)

interface Pending extends ConfirmOptions {
  resolve: (v: boolean) => void
}

export function ConfirmProvider({ children }: { children: React.ReactNode }) {
  const { t } = useT()
  const [pending, setPending] = useState<Pending | null>(null)
  const confirmBtnRef = useRef<HTMLButtonElement>(null)

  const confirm = useCallback<ConfirmFn>((opts) => {
    return new Promise<boolean>((resolve) => setPending({ ...opts, resolve }))
  }, [])

  const close = useCallback((result: boolean) => {
    setPending((p) => {
      p?.resolve(result)
      return null
    })
  }, [])

  useEffect(() => {
    if (!pending) return
    confirmBtnRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        close(false)
      } else if (e.key === 'Enter') {
        e.preventDefault()
        close(true)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [pending, close])

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      {pending && (
        <div
          className="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) close(false)
          }}
          role="dialog"
          aria-modal="true"
        >
          <div className="bg-card border border-border p-6 w-full max-w-sm crt-in">
            {pending.title && <h2 className="text-lg font-semibold mb-2">{pending.title}</h2>}
            <p className="text-sm text-muted-foreground whitespace-pre-line mb-5">{pending.message}</p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => close(false)}
                className="px-4 py-2 border border-border text-muted-foreground hover:text-foreground transition-colors text-sm"
              >
                {pending.cancelLabel ?? t('common.cancel')}
              </button>
              <button
                ref={confirmBtnRef}
                onClick={() => close(true)}
                className={
                  pending.danger
                    ? 'px-4 py-2 bg-destructive text-destructive-foreground hover:opacity-90 transition-opacity text-sm font-medium'
                    : 'px-4 py-2 bg-primary text-primary-foreground hover:bg-phosphor transition-colors text-sm font-medium'
                }
              >
                {pending.confirmLabel ?? t('common.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}
    </ConfirmContext.Provider>
  )
}

export function useConfirm(): ConfirmFn {
  const ctx = useContext(ConfirmContext)
  if (!ctx) throw new Error('useConfirm must be used within ConfirmProvider')
  return ctx
}
