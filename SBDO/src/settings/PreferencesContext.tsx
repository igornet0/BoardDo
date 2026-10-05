import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import {
  formatMessage,
  messages,
  type Locale,
  type MessageKey,
} from '../i18n/messages'
import {
  applyResolvedTheme,
  loadPreferences,
  resolveTheme,
  savePreferences,
  type Preferences,
  type ThemePreference,
} from './preferences'

interface PreferencesContextValue {
  theme: ThemePreference
  locale: Locale
  resolvedTheme: 'light' | 'dark'
  setTheme: (theme: ThemePreference) => void
  setLocale: (locale: Locale) => void
  t: (key: MessageKey, vars?: Record<string, string | number>) => string
}

const PreferencesContext = createContext<PreferencesContextValue | null>(null)

export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<Preferences>(() => loadPreferences())
  const [systemDark, setSystemDark] = useState(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches,
  )

  const resolvedTheme =
    prefs.theme === 'system' ? (systemDark ? 'dark' : 'light') : prefs.theme

  useEffect(() => {
    applyResolvedTheme(resolvedTheme)
  }, [resolvedTheme])

  useEffect(() => {
    document.documentElement.lang = prefs.locale
  }, [prefs.locale])

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  const update = useCallback((patch: Partial<Preferences>) => {
    setPrefs((prev) => {
      const next = { ...prev, ...patch }
      savePreferences(next)
      if (patch.theme) applyResolvedTheme(resolveTheme(next.theme))
      return next
    })
  }, [])

  const t = useCallback(
    (key: MessageKey, vars?: Record<string, string | number>) =>
      formatMessage(messages[prefs.locale][key] ?? messages.en[key], vars),
    [prefs.locale],
  )

  const value = useMemo<PreferencesContextValue>(
    () => ({
      theme: prefs.theme,
      locale: prefs.locale,
      resolvedTheme,
      setTheme: (theme) => update({ theme }),
      setLocale: (locale) => update({ locale }),
      t,
    }),
    [prefs.theme, prefs.locale, resolvedTheme, update, t],
  )

  return (
    <PreferencesContext.Provider value={value}>
      {children}
    </PreferencesContext.Provider>
  )
}

export function usePreferences() {
  const ctx = useContext(PreferencesContext)
  if (!ctx) {
    throw new Error('usePreferences must be used within PreferencesProvider')
  }
  return ctx
}
