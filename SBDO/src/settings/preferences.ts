import type { Locale } from '../i18n/messages'

export type ThemePreference = 'light' | 'dark' | 'system'

export interface Preferences {
  theme: ThemePreference
  locale: Locale
}

const STORAGE_KEY = 'boarddo.preferences'

const DEFAULTS: Preferences = {
  theme: 'dark',
  locale: 'en',
}

export function loadPreferences(): Preferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULTS }
    const parsed = JSON.parse(raw) as Partial<Preferences>
    return {
      theme: isTheme(parsed.theme) ? parsed.theme : DEFAULTS.theme,
      locale: isLocale(parsed.locale) ? parsed.locale : DEFAULTS.locale,
    }
  } catch {
    return { ...DEFAULTS }
  }
}

export function savePreferences(prefs: Preferences) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs))
}

export function resolveTheme(theme: ThemePreference): 'light' | 'dark' {
  if (theme === 'light' || theme === 'dark') return theme
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light'
}

export function applyResolvedTheme(resolved: 'light' | 'dark') {
  document.documentElement.setAttribute('data-theme', resolved)
}

function isTheme(v: unknown): v is ThemePreference {
  return v === 'light' || v === 'dark' || v === 'system'
}

function isLocale(v: unknown): v is Locale {
  return v === 'en' || v === 'ru'
}
