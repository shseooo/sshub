import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ChevronDown, Check } from 'lucide-react'

export interface SelectOption {
  value: string
  label: string
}

/**
 * App-themed dropdown (CRT/phosphor). Replaces native <select> so both the
 * control and the option list match the rest of the UI. The list is rendered
 * in a portal with fixed positioning so it can't be clipped or covered by
 * sibling cards' stacking contexts (e.g. the `crt-in` animation). Keyboard:
 * Up/Down move, Enter/Space pick, Esc close, Home/End jump, type-ahead.
 */
export function Select({
  value,
  onChange,
  options,
  className,
  ariaLabel,
}: {
  value: string
  onChange: (value: string) => void
  options: SelectOption[]
  className?: string
  ariaLabel?: string
}) {
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const [pos, setPos] = useState<{ left: number; top: number; width: number } | null>(null)
  const btnRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const typeahead = useRef({ buffer: '', at: 0 })
  const current = options.find((o) => o.value === value)

  const openMenu = () => {
    const r = btnRef.current?.getBoundingClientRect()
    if (r) setPos({ left: r.left, top: r.bottom + 4, width: r.width })
    const i = options.findIndex((o) => o.value === value)
    setActive(i < 0 ? 0 : i)
    setOpen(true)
  }

  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node
      if (!btnRef.current?.contains(t) && !menuRef.current?.contains(t)) setOpen(false)
    }
    // A fixed-positioned menu would drift on scroll — just close it.
    const onScroll = () => setOpen(false)
    window.addEventListener('mousedown', onDown)
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onScroll)
    return () => {
      window.removeEventListener('mousedown', onDown)
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onScroll)
    }
  }, [open])

  // Keep the highlighted option in view.
  useEffect(() => {
    if (!open) return
    menuRef.current?.querySelector<HTMLElement>(`[data-idx="${active}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [open, active])

  const commit = (i: number) => {
    const opt = options[i]
    if (opt) onChange(opt.value)
    setOpen(false)
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!open) {
      if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        openMenu()
      }
      return
    }
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault()
        setActive((a) => Math.min(options.length - 1, a + 1))
        break
      case 'ArrowUp':
        e.preventDefault()
        setActive((a) => Math.max(0, a - 1))
        break
      case 'Home':
        e.preventDefault()
        setActive(0)
        break
      case 'End':
        e.preventDefault()
        setActive(options.length - 1)
        break
      case 'Enter':
      case ' ':
        e.preventDefault()
        commit(active)
        break
      case 'Escape':
        e.preventDefault()
        setOpen(false)
        break
      default:
        if (e.key.length === 1) {
          const now = Date.now()
          const ta = typeahead.current
          ta.buffer = now - ta.at > 600 ? e.key : ta.buffer + e.key
          ta.at = now
          const q = ta.buffer.toLowerCase()
          const idx = options.findIndex((o) => o.label.toLowerCase().startsWith(q))
          if (idx >= 0) setActive(idx)
        }
    }
  }

  return (
    <div className={`relative ${className ?? ''}`}>
      <button
        ref={btnRef}
        type="button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => (open ? setOpen(false) : openMenu())}
        onKeyDown={onKeyDown}
        className={`w-full flex items-center justify-between gap-2 px-3 py-2 bg-background border text-sm text-left transition-colors focus:outline-hidden ${
          open ? 'border-phosphor/60 ring-1 ring-phosphor/40' : 'border-border hover:border-phosphor/40'
        }`}
      >
        <span className="truncate">{current?.label ?? ''}</span>
        <ChevronDown
          className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${open ? 'rotate-180 text-phosphor' : ''}`}
        />
      </button>

      {open &&
        pos &&
        createPortal(
          <div
            ref={menuRef}
            role="listbox"
            style={{ position: 'fixed', left: pos.left, top: pos.top, width: pos.width }}
            className="max-h-60 overflow-y-auto bg-popover border border-border shadow-lg z-[100] crt-in"
          >
            {options.map((opt, i) => {
              const selected = opt.value === value
              return (
                <button
                  key={opt.value}
                  type="button"
                  role="option"
                  data-idx={i}
                  aria-selected={selected}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => commit(i)}
                  className={`w-full flex items-center justify-between gap-2 px-3 py-2 text-xs text-left transition-colors ${
                    i === active ? 'bg-accent text-phosphor' : 'text-foreground hover:bg-accent hover:text-accent-foreground'
                  }`}
                >
                  <span className="truncate">{opt.label}</span>
                  {selected && <Check className="h-3.5 w-3.5 shrink-0 text-phosphor" />}
                </button>
              )
            })}
          </div>,
          document.body
        )}
    </div>
  )
}
