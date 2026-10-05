import type { Locale } from '../i18n/messages'

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
/** Modifier key label for shortcut hints. */
export const MOD = isMac ? '⌘' : 'Ctrl+'

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['week', 7 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
  ['second', 1],
]

/** "5 minutes ago" / "через 2 часа" — falls back to the raw string. */
export function formatRelative(iso: string | null | undefined, locale: Locale): string {
  if (!iso) return '—'
  const ts = new Date(iso).getTime()
  if (Number.isNaN(ts)) return iso
  const diff = Math.round((ts - Date.now()) / 1000)
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  for (const [unit, secs] of UNITS) {
    if (Math.abs(diff) >= secs || unit === 'second') {
      return rtf.format(Math.round(diff / secs), unit)
    }
  }
  return iso
}

export function formatDateTime(iso: string | null | undefined, locale: Locale): string {
  if (!iso) return '—'
  try {
    return new Date(iso).toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
  } catch {
    return iso
  }
}

export function formatTime(iso: string | null | undefined, locale: Locale): string {
  if (!iso) return '—'
  try {
    return new Date(iso).toLocaleTimeString(locale, {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    })
  } catch {
    return iso
  }
}
