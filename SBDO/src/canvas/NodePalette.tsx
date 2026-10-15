import { useMemo, useState } from 'react'
import {
  INTEGRATION_GROUPS,
  PALETTE,
  availablePalette,
  integrationGroupFor,
  type IntegrationGroup,
  type PaletteItem,
} from './paletteItems'
import { useExtensions } from '../extensions/ExtensionsContext'
import { usePreferences } from '../settings/PreferencesContext'
import { SearchInput } from '../ui/primitives'
import { MOD } from '../ui/format'
import {
  IconChevron,
  IconForType,
  visualCategoryForType,
  type NodeVisualCategory,
} from '../ui/icons'
import type { MessageKey } from '../i18n/messages'

interface Props {
  onAdd: (item: PaletteItem) => void
  onOpenSearch?: () => void
  onClose?: () => void
}

type Section = NodeVisualCategory | IntegrationGroup

/** Core categories first, then one section per integration, then other integrations. */
const SECTION_ORDER: Section[] = [
  'trigger',
  'logic',
  'data',
  'ai',
  'action',
  ...INTEGRATION_GROUPS,
  'integration',
]

const CATEGORY_KEYS: Record<NodeVisualCategory, MessageKey> = {
  trigger: 'palette.nav.triggers',
  logic: 'palette.nav.logic',
  data: 'palette.nav.data',
  ai: 'palette.nav.ai',
  action: 'palette.nav.actions',
  integration: 'palette.nav.integrations',
}

export function NodePalette({ onAdd, onOpenSearch, onClose }: Props) {
  const { t } = usePreferences()
  const [query, setQuery] = useState('')
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(SECTION_ORDER.map((c) => [c, true])),
  )

  const { projecthubReady } = useExtensions()
  const items = useMemo(() => availablePalette(PALETTE, projecthubReady), [projecthubReady])
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return items
    return items.filter((item) => {
      const label = t(item.labelKey).toLowerCase()
      return label.includes(q) || item.typeId.toLowerCase().includes(q)
    })
  }, [query, t, items])

  const bySection = useMemo(() => {
    const map = new Map<Section, PaletteItem[]>()
    for (const section of SECTION_ORDER) map.set(section, [])
    for (const item of filtered) {
      const section = integrationGroupFor(item) ?? visualCategoryForType(item.typeId)
      map.get(section)?.push(item)
    }
    return map
  }, [filtered])

  return (
    <aside className="palette">
      <div className="palette__header">
        <h2 className="palette__title">{t('palette.title')}</h2>
        <div className="palette__header-actions">
          <button
            type="button"
            className="kbd-btn"
            onClick={onOpenSearch}
            data-tip={t('cmd.searchNodes')}
          >
            {MOD}K
          </button>
          {onClose && (
            <button type="button" className="icon-btn icon-btn--sm" onClick={onClose} aria-label={t('common.close')}>
              <IconChevron size={14} style={{ transform: 'rotate(90deg)' }} />
            </button>
          )}
        </div>
      </div>

      <div className="palette__search">
        <SearchInput value={query} onChange={setQuery} placeholder={t('palette.search')} />
      </div>
      <p className="palette__tip">{t('palette.tip')}</p>

      <div className="palette__groups">
        {SECTION_ORDER.map((section) => {
          const items = bySection.get(section) ?? []
          if (!items.length) return null
          const isOpen = openGroups[section] ?? true
          const title = section in CATEGORY_KEYS ? t(CATEGORY_KEYS[section as NodeVisualCategory]) : t(section as MessageKey)
          return (
            <div key={section} className="palette__group">
              <button
                type="button"
                className="palette__group-toggle"
                onClick={() =>
                  setOpenGroups((g) => ({ ...g, [section]: !isOpen }))
                }
                aria-expanded={isOpen}
              >
                <IconChevron
                  size={12}
                  style={{
                    transform: isOpen ? undefined : 'rotate(-90deg)',
                  }}
                />
                <span>{title}</span>
                <span className="palette__group-count">{items.length}</span>
              </button>
              {isOpen &&
                items.map((item) => {
                  const cat = visualCategoryForType(item.typeId)
                  return (
                  <button
                    key={item.typeId}
                    type="button"
                    className={`palette__item palette__item--${cat}`}
                    draggable
                    onDragStart={(e) => {
                      e.dataTransfer.setData(
                        'application/boarddo-node',
                        item.typeId,
                      )
                      e.dataTransfer.effectAllowed = 'move'
                    }}
                    onClick={() => onAdd(item)}
                  >
                    <span className={`palette__item-icon palette__item-icon--${cat}`}>
                      <IconForType typeId={item.typeId} size={13} />
                    </span>
                    <span className="palette__item-label">{t(item.labelKey)}</span>
                    <span className="palette__item-add" aria-hidden>+</span>
                  </button>
                  )
                })}
            </div>
          )
        })}
        {filtered.length === 0 && <p className="palette__empty">{t('cmd.empty')}</p>}
      </div>
    </aside>
  )
}
