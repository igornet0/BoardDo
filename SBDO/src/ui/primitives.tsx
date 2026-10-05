import type { ReactNode } from 'react'
import { IconAlert, IconCheck, IconInfo, IconSearch } from './icons'

export type Tone = 'neutral' | 'info' | 'success' | 'warning' | 'error' | 'ai'

/** Most panel messages are free text (`String(err)` or a success line). */
export function toneOf(text: string): Tone {
  if (/error|fail|unable|denied|invalid|conflict|ошибк|не удалось|409|500/i.test(text)) {
    return 'error'
  }
  if (/^ok\b|saved|created|deleted|updated|started|stopped|reloaded|opened|loaded|сохран|создан|удал|обновл|запущ|остановл|открыт|загруж/i.test(text)) {
    return 'success'
  }
  return 'info'
}

export function Notice({
  text,
  tone,
  onDismiss,
}: {
  text: string | null | undefined
  tone?: Tone
  onDismiss?: () => void
}) {
  if (!text) return null
  const resolved = tone ?? toneOf(text)
  const icon =
    resolved === 'error' || resolved === 'warning' ? (
      <IconAlert size={15} />
    ) : resolved === 'success' ? (
      <IconCheck size={15} />
    ) : (
      <IconInfo size={15} />
    )
  return (
    <div className={`notice notice--${resolved}`} role={resolved === 'error' ? 'alert' : 'status'}>
      <span className="notice__icon">{icon}</span>
      <span className="notice__text">{text}</span>
      {onDismiss && (
        <button type="button" className="notice__close" onClick={onDismiss} aria-label="Dismiss">
          ×
        </button>
      )}
    </div>
  )
}

export function Badge({
  tone = 'neutral',
  dot,
  children,
  title,
}: {
  tone?: Tone
  dot?: boolean
  children: ReactNode
  title?: string
}) {
  return (
    <span className={`badge badge--${tone}`} title={title}>
      {dot && <span className="badge__dot" aria-hidden />}
      {children}
    </span>
  )
}

export function EmptyState({
  icon,
  title,
  hint,
  action,
  compact,
}: {
  icon?: ReactNode
  title: ReactNode
  hint?: ReactNode
  action?: ReactNode
  compact?: boolean
}) {
  return (
    <div className={`empty${compact ? ' empty--compact' : ''}`}>
      {icon && <span className="empty__icon">{icon}</span>}
      <strong className="empty__title">{title}</strong>
      {hint && <p className="empty__hint">{hint}</p>}
      {action && <div className="empty__action">{action}</div>}
    </div>
  )
}

export function SearchInput({
  value,
  onChange,
  placeholder,
  autoFocus,
}: {
  value: string
  onChange: (v: string) => void
  placeholder?: string
  autoFocus?: boolean
}) {
  return (
    <label className="search-input">
      <IconSearch size={14} />
      <input
        value={value}
        autoFocus={autoFocus}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        aria-label={placeholder}
      />
      {value && (
        <button type="button" className="search-input__clear" onClick={() => onChange('')} aria-label="Clear">
          ×
        </button>
      )}
    </label>
  )
}

export interface TabItem<T extends string> {
  id: T
  label: ReactNode
  icon?: ReactNode
  count?: number
  alert?: boolean
}

/** Underline tabs — scrolls horizontally when there are many. */
export function Tabs<T extends string>({
  items,
  value,
  onChange,
  variant = 'line',
}: {
  items: TabItem<T>[]
  value: T
  onChange: (id: T) => void
  variant?: 'line' | 'pill'
}) {
  return (
    <div className={`tabs tabs--${variant}`} role="tablist">
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          role="tab"
          aria-selected={value === item.id}
          className={`tabs__tab${value === item.id ? ' is-active' : ''}`}
          onClick={() => onChange(item.id)}
        >
          {item.icon}
          <span>{item.label}</span>
          {item.count != null && (
            <span className={`tabs__count${item.alert ? ' tabs__count--alert' : ''}`}>
              {item.count}
            </span>
          )}
        </button>
      ))}
    </div>
  )
}

/** Compact segmented control for 2–5 mutually exclusive options. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { value: T; label: ReactNode; icon?: ReactNode }[]
  value: T
  onChange: (v: T) => void
}) {
  return (
    <div className="segmented" role="radiogroup">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          className={`segmented__opt${value === o.value ? ' is-active' : ''}`}
          onClick={() => onChange(o.value)}
        >
          {o.icon}
          {o.label}
        </button>
      ))}
    </div>
  )
}

export function Stat({
  label,
  value,
  hint,
  icon,
  tone = 'neutral',
}: {
  label: ReactNode
  value: ReactNode
  hint?: ReactNode
  icon?: ReactNode
  tone?: Tone
}) {
  return (
    <div className={`stat stat--${tone}`}>
      <div className="stat__head">
        {icon && <span className="stat__icon">{icon}</span>}
        <span className="stat__label">{label}</span>
      </div>
      <strong className="stat__value">{value}</strong>
      {hint && <span className="stat__hint">{hint}</span>}
    </div>
  )
}

export function Field({
  label,
  hint,
  children,
  span,
}: {
  label: ReactNode
  hint?: ReactNode
  children: ReactNode
  span?: 'full'
}) {
  return (
    <label className={`field${span === 'full' ? ' field--full' : ''}`}>
      <span className="field__label">{label}</span>
      {children}
      {hint && <span className="field__hint">{hint}</span>}
    </label>
  )
}

export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  label: ReactNode
}) {
  return (
    <label className="switch">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="switch__track" aria-hidden>
        <span className="switch__thumb" />
      </span>
      <span className="switch__label">{label}</span>
    </label>
  )
}

export function Card({
  title,
  subtitle,
  actions,
  children,
  className,
}: {
  title?: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    <section className={`card${className ? ` ${className}` : ''}`}>
      {(title || actions) && (
        <header className="card__header">
          <div>
            {title && <h3 className="card__title">{title}</h3>}
            {subtitle && <p className="card__subtitle">{subtitle}</p>}
          </div>
          {actions && <div className="card__actions">{actions}</div>}
        </header>
      )}
      {children}
    </section>
  )
}

export function Progress({ value, tone = 'brand' }: { value: number; tone?: 'brand' | 'success' }) {
  const pct = Math.max(0, Math.min(100, value))
  return (
    <div className={`progress progress--${tone}`} role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
      <span style={{ width: `${pct}%` }} />
    </div>
  )
}

/** First letter avatar with a hue derived from the text. */
export function Avatar({ text, size = 32 }: { text: string; size?: number }) {
  let hash = 0
  for (const ch of text) hash = (hash * 31 + ch.charCodeAt(0)) % 360
  return (
    <span
      className="avatar"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.42,
        background: `linear-gradient(135deg, hsl(${hash} 70% 55%), hsl(${(hash + 40) % 360} 70% 45%))`,
      }}
      aria-hidden
    >
      {(text.trim()[0] ?? '?').toUpperCase()}
    </span>
  )
}
