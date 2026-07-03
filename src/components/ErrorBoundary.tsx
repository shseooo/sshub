import { Component, type ReactNode } from 'react'
import { detectLang, translate } from '@/i18n'

// The one place a class component is required: React has no hook API for error
// boundaries (getDerivedStateFromError / componentDidCatch are class-only).
// Without this, any render exception unmounts the whole tree and leaves a blank
// window with no way to recover. The live PTY sessions live in the main process,
// so a reload re-mounts the UI and reconnects them.

interface State {
  error: Error | null
}

export default class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    console.error('Render error caught by ErrorBoundary:', error, info.componentStack)
  }

  render() {
    if (!this.state.error) return this.props.children

    // Translate without the Language context — it may be above or unavailable
    // when we render the fallback.
    const lang = detectLang()
    const t = (key: string) => translate(lang, key)

    return (
      <div className="flex h-screen flex-col items-center justify-center gap-4 bg-background p-8 text-center text-foreground">
        <h1 className="text-lg text-phosphor">{t('error.title')}</h1>
        <p className="max-w-md text-sm opacity-80">{t('error.message')}</p>
        <pre className="max-w-full overflow-x-auto rounded border border-border p-2 text-left text-xs opacity-60">
          {this.state.error.message}
        </pre>
        <button
          className="border border-border px-4 py-1.5 text-sm text-phosphor hover:bg-border/30"
          onClick={() => location.reload()}
        >
          {t('error.reload')}
        </button>
      </div>
    )
  }
}
