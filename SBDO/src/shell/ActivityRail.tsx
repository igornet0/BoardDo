import type { ReactNode } from 'react'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import {
  IconActivity,
  IconFolder,
  IconMoon,
  IconPlug,
  IconRadio,
  IconSend,
  IconSettings,
  IconSidebar,
  IconSparkle,
  IconSun,
  IconTarget,
  IconTemplate,
  IconWrench,
} from '../ui/icons'

export type RailTarget =
  | 'projects'
  | 'scenarios'
  | 'goals'
  | 'runtime'
  | 'ingress'
  | 'connections'
  | 'tools'
  | 'telegram'
  | 'settings'

interface Props {
  active: RailTarget | null
  sidebarOpen: boolean
  onToggleSidebar: () => void
  onOpen: (target: RailTarget) => void
  liveRuntimes: number
  agentReady: boolean
  agentOpen: boolean
  onOpenAgent: () => void
}

const MAIN: { id: RailTarget; key: MessageKey; icon: ReactNode }[] = [
  { id: 'projects', key: 'nav.projects', icon: <IconFolder size={18} /> },
  { id: 'scenarios', key: 'nav.scenarios', icon: <IconTemplate size={18} /> },
  { id: 'goals', key: 'nav.goals', icon: <IconTarget size={18} /> },
  { id: 'runtime', key: 'nav.runtime', icon: <IconActivity size={18} /> },
]

const INTEGRATIONS: { id: RailTarget; key: MessageKey; icon: ReactNode }[] = [
  { id: 'ingress', key: 'nav.ingress', icon: <IconRadio size={18} /> },
  { id: 'connections', key: 'nav.connections', icon: <IconPlug size={18} /> },
  { id: 'telegram', key: 'nav.telegram', icon: <IconSend size={18} /> },
  { id: 'tools', key: 'nav.tools', icon: <IconWrench size={18} /> },
]

export function ActivityRail({
  active,
  sidebarOpen,
  onToggleSidebar,
  onOpen,
  liveRuntimes,
  agentReady,
  agentOpen,
  onOpenAgent,
}: Props) {
  const { t, resolvedTheme, setTheme } = usePreferences()

  function item(entry: { id: RailTarget; key: MessageKey; icon: ReactNode }) {
    const badge = entry.id === 'runtime' && liveRuntimes > 0 ? liveRuntimes : null
    return (
      <button
        key={entry.id}
        type="button"
        className={`rail__btn${active === entry.id ? ' is-active' : ''}`}
        onClick={() => onOpen(entry.id)}
        data-tip={t(entry.key)}
        aria-label={t(entry.key)}
      >
        {entry.icon}
        {badge != null && <span className="rail__badge">{badge}</span>}
      </button>
    )
  }

  return (
    <nav className="rail" aria-label={t('nav.label')}>
      <button
        type="button"
        className={`rail__btn${sidebarOpen ? ' is-on' : ''}`}
        onClick={onToggleSidebar}
        data-tip={t('nav.editor')}
        aria-label={t('nav.editor')}
        aria-pressed={sidebarOpen}
      >
        <IconSidebar size={18} />
      </button>
      <span className="rail__sep" />
      {MAIN.map(item)}
      <span className="rail__sep" />
      {INTEGRATIONS.map(item)}

      <div className="rail__spacer" />

      <span className="rail__sep" />
      <button
        type="button"
        className={`rail__ai${agentReady ? ' is-ready' : ''}${agentOpen ? ' is-active' : ''}`}
        onClick={onOpenAgent}
        data-tip={t('nav.agent')}
        aria-label={t('nav.agent')}
        aria-pressed={agentOpen}
      >
        <IconSparkle size={18} />
        {!agentReady && <span className="rail__ai-dot" aria-hidden />}
      </button>
      <button
        type="button"
        className={`rail__theme rail__theme--${resolvedTheme}`}
        onClick={() => setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')}
        data-tip={resolvedTheme === 'dark' ? t('settings.theme.light') : t('settings.theme.dark')}
        aria-label={t('settings.theme')}
      >
        <span className="rail__theme-thumb" aria-hidden />
        <span className={`rail__theme-opt${resolvedTheme === 'light' ? ' is-on' : ''}`}>
          <IconSun size={13} />
        </span>
        <span className={`rail__theme-opt${resolvedTheme === 'dark' ? ' is-on' : ''}`}>
          <IconMoon size={13} />
        </span>
      </button>
      {item({ id: 'settings', key: 'nav.settings', icon: <IconSettings size={18} /> })}
    </nav>
  )
}
