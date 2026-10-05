import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { PALETTE, type PaletteItem } from '../canvas/paletteItems'
import { usePreferences } from '../settings/PreferencesContext'
import { MOD } from './format'
import {
  IconActivity,
  IconCommand,
  IconFlask,
  IconFolder,
  IconForType,
  IconPlay,
  IconPlug,
  IconPlus,
  IconRadio,
  IconSave,
  IconSearch,
  IconSend,
  IconSettings,
  IconSparkle,
  IconTarget,
  IconTemplate,
  IconTerminal,
  IconWrench,
  visualCategoryForType,
} from './icons'

export type CommandAction =
  | { kind: 'add-node'; item: PaletteItem }
  | { kind: 'run' }
  | { kind: 'test' }
  | { kind: 'save' }
  | { kind: 'projects' }
  | { kind: 'scenarios' }
  | { kind: 'goals' }
  | { kind: 'tools' }
  | { kind: 'runtime' }
  | { kind: 'settings' }
  | { kind: 'connections' }
  | { kind: 'telegram' }
  | { kind: 'ingress' }
  | { kind: 'new-board' }
  | { kind: 'fit' }
  | { kind: 'toggle-bottom' }
  | { kind: 'agent' }
  | { kind: 'agent-settings' }

interface Props {
  open: boolean
  onClose: () => void
  onAction: (action: CommandAction) => void
  initialQuery?: string
  mode?: 'command' | 'nodes'
}

interface Entry {
  id: string
  label: string
  hint?: string
  group: string
  action: CommandAction
  icon?: ReactNode
}

export function CommandPalette({
  open,
  onClose,
  onAction,
  initialQuery = '',
  mode = 'command',
}: Props) {
  const { t } = usePreferences()
  const [query, setQuery] = useState(initialQuery)
  const [active, setActive] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!open) return
    setQuery(initialQuery)
    setActive(0)
    const id = window.setTimeout(() => inputRef.current?.focus(), 20)
    return () => window.clearTimeout(id)
  }, [open, initialQuery])

  const entries = useMemo(() => {
    const q = query.trim().toLowerCase()
    const nodeEntries: Entry[] = PALETTE.map((item) => ({
      id: `node:${item.typeId}`,
      label: t(item.labelKey),
      hint: item.typeId,
      group: t('cmd.group.nodes'),
      action: { kind: 'add-node', item },
      icon: (
        <span className={`palette__item-icon palette__item-icon--${visualCategoryForType(item.typeId)}`}>
          <IconForType typeId={item.typeId} size={12} />
        </span>
      ),
    }))

    const appEntries: Entry[] =
      mode === 'nodes'
        ? []
        : [
            {
              id: 'run',
              icon: <IconPlay size={14} />,
              label: t('cmd.run'),
              hint: 'R',
              group: t('cmd.group.workflow'),
              action: { kind: 'run' },
            },
            {
              id: 'test',
              icon: <IconFlask size={14} />,
              label: t('cmd.test'),
              hint: 'T',
              group: t('cmd.group.workflow'),
              action: { kind: 'test' },
            },
            {
              id: 'save',
              icon: <IconSave size={14} />,
              label: t('cmd.save'),
              hint: `${MOD}S`,
              group: t('cmd.group.workflow'),
              action: { kind: 'save' },
            },
            {
              id: 'projects',
              icon: <IconFolder size={14} />,
              label: t('app.projects'),
              group: t('cmd.group.navigate'),
              action: { kind: 'projects' },
            },
            {
              id: 'scenarios',
              icon: <IconTemplate size={14} />,
              label: t('app.scenarios'),
              group: t('cmd.group.navigate'),
              action: { kind: 'scenarios' },
            },
            {
              id: 'goals',
              icon: <IconTarget size={14} />,
              label: t('app.goals'),
              group: t('cmd.group.navigate'),
              action: { kind: 'goals' },
            },
            {
              id: 'tools',
              icon: <IconWrench size={14} />,
              label: t('app.tools'),
              group: t('cmd.group.navigate'),
              action: { kind: 'tools' },
            },
            {
              id: 'runtime',
              icon: <IconActivity size={14} />,
              label: t('runtime.title'),
              group: t('cmd.group.navigate'),
              action: { kind: 'runtime' },
            },
            {
              id: 'settings',
              icon: <IconSettings size={14} />,
              label: t('app.settings'),
              group: t('cmd.group.navigate'),
              action: { kind: 'settings' },
            },
            {
              id: 'agent',
              icon: <IconSparkle size={14} />,
              label: t('cmd.agent'),
              group: t('cmd.group.navigate'),
              action: { kind: 'agent' },
            },
            {
              id: 'agent-settings',
              icon: <IconSparkle size={14} />,
              label: t('cmd.agentSettings'),
              group: t('cmd.group.navigate'),
              action: { kind: 'agent-settings' },
            },
            {
              id: 'connections',
              icon: <IconPlug size={14} />,
              label: t('app.connections'),
              group: t('cmd.group.navigate'),
              action: { kind: 'connections' },
            },
            {
              id: 'telegram',
              icon: <IconSend size={14} />,
              label: t('app.telegram'),
              group: t('cmd.group.navigate'),
              action: { kind: 'telegram' },
            },
            {
              id: 'ingress',
              icon: <IconRadio size={14} />,
              label: t('app.ingress'),
              group: t('cmd.group.navigate'),
              action: { kind: 'ingress' },
            },
            {
              id: 'new',
              icon: <IconPlus size={14} />,
              label: t('app.newBoard'),
              group: t('cmd.group.workflow'),
              action: { kind: 'new-board' },
            },
            {
              id: 'bottom',
              icon: <IconTerminal size={14} />,
              label: t('cmd.toggleBottom'),
              group: t('cmd.group.workflow'),
              action: { kind: 'toggle-bottom' },
            },
          ]

    const all = mode === 'nodes' ? nodeEntries : [...appEntries, ...nodeEntries]
    if (!q) return all
    return all.filter(
      (e) =>
        e.label.toLowerCase().includes(q) ||
        (e.hint?.toLowerCase().includes(q) ?? false) ||
        e.group.toLowerCase().includes(q),
    )
  }, [query, t, mode])

  useEffect(() => {
    setActive(0)
  }, [query, open])

  if (!open) return null

  function run(entry: Entry) {
    onAction(entry.action)
    onClose()
  }

  const groups = [...new Set(entries.map((e) => e.group))]

  return (
    <div
      className="cmd-overlay"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="cmd-palette"
        role="dialog"
        aria-modal="true"
        aria-label={t('cmd.title')}
      >
        <div className="cmd-palette__search">
          <IconSearch size={16} />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={
              mode === 'nodes' ? t('cmd.searchNodes') : t('cmd.placeholder')
            }
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.preventDefault()
                onClose()
              } else if (e.key === 'ArrowDown') {
                e.preventDefault()
                setActive((i) => Math.min(i + 1, entries.length - 1))
              } else if (e.key === 'ArrowUp') {
                e.preventDefault()
                setActive((i) => Math.max(i - 1, 0))
              } else if (e.key === 'Enter' && entries[active]) {
                e.preventDefault()
                run(entries[active])
              }
            }}
          />
          <kbd className="cmd-palette__kbd">esc</kbd>
        </div>
        <div className="cmd-palette__list">
          {entries.length === 0 && (
            <div className="cmd-palette__empty">{t('cmd.empty')}</div>
          )}
          {groups.map((group) => {
            const items = entries.filter((e) => e.group === group)
            if (!items.length) return null
            return (
              <div key={group} className="cmd-palette__group">
                <div className="cmd-palette__group-label">{group}</div>
                {items.map((entry) => {
                  const idx = entries.indexOf(entry)
                  return (
                    <button
                      key={entry.id}
                      type="button"
                      className={`cmd-palette__item${idx === active ? ' is-active' : ''}`}
                      ref={idx === active ? (el) => el?.scrollIntoView({ block: 'nearest' }) : undefined}
                      onMouseMove={() => idx !== active && setActive(idx)}
                      onClick={() => run(entry)}
                    >
                      <span className="cmd-palette__item-icon">
                        {entry.icon ?? <IconCommand size={13} />}
                      </span>
                      <span className="cmd-palette__item-label">
                        {entry.label}
                      </span>
                      {entry.hint && (
                        <span className="cmd-palette__item-hint">{entry.hint}</span>
                      )}
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
        <div className="cmd-palette__footer">
          <span><kbd>↑</kbd><kbd>↓</kbd> {t('cmd.navigate')}</span>
          <span><kbd>↵</kbd> {t('cmd.select')}</span>
          <span><kbd>esc</kbd> {t('common.close')}</span>
        </div>
      </div>
    </div>
  )
}
