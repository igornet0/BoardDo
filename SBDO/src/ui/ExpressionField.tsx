import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type RefObject,
} from 'react'
import { usePreferences } from '../settings/PreferencesContext'

const SUGGESTIONS = [
  'trigger',
  'trigger.text',
  'trigger.chat_id',
  'trigger.account_id',
  'trigger.message_id',
  'variables',
  'nodes',
  'execution',
]

interface Props {
  value: string
  onChange: (value: string) => void
  multiline?: boolean
  rows?: number
  placeholder?: string
  className?: string
  suggestions?: string[]
}

function tokenize(value: string): { text: string; expr: boolean }[] {
  const parts: { text: string; expr: boolean }[] = []
  const re = /\{\{[^}]*\}\}/g
  let last = 0
  let m: RegExpExecArray | null
  while ((m = re.exec(value))) {
    if (m.index > last) {
      parts.push({ text: value.slice(last, m.index), expr: false })
    }
    parts.push({ text: m[0], expr: true })
    last = m.index + m[0].length
  }
  if (last < value.length) parts.push({ text: value.slice(last), expr: false })
  if (!parts.length) parts.push({ text: value, expr: false })
  return parts
}

export function ExpressionField({
  value,
  onChange,
  multiline = false,
  rows = 3,
  placeholder,
  className = '',
  suggestions = SUGGESTIONS,
}: Props) {
  const { t } = usePreferences()
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const ref = useRef<HTMLTextAreaElement | HTMLInputElement>(null)

  const incomplete = useMemo(() => {
    const openIdx = value.lastIndexOf('{{')
    const closeIdx = value.lastIndexOf('}}')
    if (openIdx === -1) return null
    if (closeIdx > openIdx) return null
    return value.slice(openIdx + 2)
  }, [value])

  const filtered = useMemo(() => {
    if (incomplete == null) return []
    const q = incomplete.trim().toLowerCase()
    return suggestions.filter((s) => !q || s.toLowerCase().startsWith(q))
  }, [incomplete, suggestions])

  useEffect(() => {
    setOpen(incomplete != null && filtered.length > 0)
    setActive(0)
  }, [incomplete, filtered.length])

  function insertSuggestion(s: string) {
    const openIdx = value.lastIndexOf('{{')
    if (openIdx === -1) return
    const next = `${value.slice(0, openIdx)}{{${s}}}`
    onChange(next)
    setOpen(false)
    ref.current?.focus()
  }

  const sharedProps = {
    className: `expr-field__input ${className}`.trim(),
    value,
    placeholder,
    spellCheck: false as const,
    onChange: (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
      onChange(e.target.value),
    onKeyDown: (e: KeyboardEvent) => {
      if (!open || !filtered.length) return
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActive((i) => Math.min(i + 1, filtered.length - 1))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActive((i) => Math.max(i - 1, 0))
      } else if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault()
        insertSuggestion(filtered[active])
      } else if (e.key === 'Escape') {
        setOpen(false)
      }
    },
    onBlur: () => {
      window.setTimeout(() => setOpen(false), 120)
    },
  }

  const tokens = tokenize(value)
  const hasExpr = tokens.some((p) => p.expr)

  return (
    <div className="expr-field">
      {multiline ? (
        <textarea
          ref={ref as RefObject<HTMLTextAreaElement>}
          rows={rows}
          {...sharedProps}
        />
      ) : (
        <input ref={ref as RefObject<HTMLInputElement>} {...sharedProps} />
      )}
      {hasExpr && (
        <div className="expr-field__tokens" aria-hidden>
          {tokens.map((p, i) =>
            p.expr ? (
              <span key={i} className="expr-token">
                {p.text}
              </span>
            ) : p.text.trim() ? (
              <span key={i} className="expr-field__plain">
                {p.text.length > 24 ? `${p.text.slice(0, 24)}…` : p.text}
              </span>
            ) : null,
          )}
        </div>
      )}
      {open && (
        <div className="expr-field__suggest" role="listbox">
          <div className="expr-field__suggest-label">{t('expr.suggestions')}</div>
          {filtered.map((s, i) => (
            <button
              key={s}
              type="button"
              role="option"
              aria-selected={i === active}
              className={i === active ? 'is-active' : ''}
              onMouseDown={(e) => {
                e.preventDefault()
                insertSuggestion(s)
              }}
            >
              <code>{`{{${s}}}`}</code>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
