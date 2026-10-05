import { useMemo, useState } from 'react'
import { BUILTIN_SCENARIOS } from '../scenarios'
import type { ScenarioTag, ScenarioTemplate } from '../scenarios'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import { Modal } from '../ui/Modal'
import { EmptyState, SearchInput } from '../ui/primitives'
import { labelKeyForType } from '../canvas/paletteItems'
import {
  IconArrowRight,
  IconForType,
  IconInfo,
  IconTemplate,
  visualCategoryForType,
} from '../ui/icons'

interface Props {
  open: boolean
  onClose: () => void
  onUse: (scenario: ScenarioTemplate) => void
}

const TAG_FILTERS: { id: 'all' | ScenarioTag; key: MessageKey }[] = [
  { id: 'all', key: 'scenarios.filter.all' },
  { id: 'starter', key: 'scenarios.filter.starter' },
  { id: 'telegram', key: 'scenarios.filter.telegram' },
  { id: 'ai', key: 'scenarios.filter.ai' },
  { id: 'http', key: 'scenarios.filter.http' },
  { id: 'github', key: 'scenarios.filter.github' },
  { id: 'logic', key: 'scenarios.filter.logic' },
]

/** Distinct node types in order — a mini "recipe" preview for the card. */
function flowTypes(s: ScenarioTemplate): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const n of s.definition.nodes) {
    if (!seen.has(n.type_id)) {
      seen.add(n.type_id)
      out.push(n.type_id)
    }
  }
  return out
}

export function ScenariosPanel({ open, onClose, onUse }: Props) {
  const { t } = usePreferences()
  const [filter, setFilter] = useState<'all' | ScenarioTag>('all')
  const [query, setQuery] = useState('')
  const [selectedId, setSelectedId] = useState<string | null>(BUILTIN_SCENARIOS[0]?.id ?? null)

  const items = useMemo(() => {
    const q = query.trim().toLowerCase()
    return BUILTIN_SCENARIOS.filter(
      (s) =>
        (filter === 'all' || s.tags.includes(filter)) &&
        (!q ||
          t(s.nameKey).toLowerCase().includes(q) ||
          t(s.descriptionKey).toLowerCase().includes(q)),
    )
  }, [filter, query, t])

  const selected = items.find((s) => s.id === selectedId) ?? items[0] ?? null

  function use(s: ScenarioTemplate) {
    onUse(s)
    onClose()
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      flush
      icon={<IconTemplate size={18} />}
      title={t('scenarios.title')}
      subtitle={t('scenarios.hint')}
    >
      <div className="split">
        <div className="split__main">
          <div className="toolbar toolbar--wrap">
            <SearchInput value={query} onChange={setQuery} placeholder={t('scenarios.search')} autoFocus />
          </div>
          <div className="chip-row">
            {TAG_FILTERS.map((f) => {
              const count =
                f.id === 'all'
                  ? BUILTIN_SCENARIOS.length
                  : BUILTIN_SCENARIOS.filter((s) => s.tags.includes(f.id as ScenarioTag)).length
              return (
                <button
                  key={f.id}
                  type="button"
                  className={`filter-chip${filter === f.id ? ' is-active' : ''}`}
                  onClick={() => setFilter(f.id)}
                >
                  {t(f.key)}
                  <span>{count}</span>
                </button>
              )
            })}
          </div>

          {items.length === 0 ? (
            <EmptyState compact title={t('scenarios.empty')} />
          ) : (
            <div className="card-grid card-grid--tight">
              {items.map((item) => {
                const types = flowTypes(item)
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={`template-card${selected?.id === item.id ? ' is-selected' : ''}`}
                    onClick={() => setSelectedId(item.id)}
                    onDoubleClick={() => use(item)}
                  >
                    <div className="template-card__flow">
                      {types.slice(0, 5).map((typeId, i) => (
                        <span key={typeId} className="template-card__step">
                          {i > 0 && <span className="template-card__arrow" />}
                          <span className={`palette__item-icon palette__item-icon--${visualCategoryForType(typeId)}`}>
                            <IconForType typeId={typeId} size={12} />
                          </span>
                        </span>
                      ))}
                      {types.length > 5 && <span className="template-card__more">+{types.length - 5}</span>}
                    </div>
                    <strong>{t(item.nameKey)}</strong>
                    <p>{t(item.descriptionKey)}</p>
                    <span className="template-card__tags">
                      {item.tags.map((tag) => (
                        <span key={tag} className="chip">
                          {t(`scenarios.filter.${tag}` as MessageKey)}
                        </span>
                      ))}
                    </span>
                  </button>
                )
              })}
            </div>
          )}
        </div>

        <aside className="split__side">
          {selected ? (
            <div className="template-detail">
              <span className="eyebrow">{t('scenarios.preview')}</span>
              <h3>{t(selected.nameKey)}</h3>
              <p className="template-detail__desc">{t(selected.descriptionKey)}</p>
              {selected.setupHintKey && (
                <div className="callout callout--info">
                  <IconInfo size={15} />
                  <span>{t(selected.setupHintKey)}</span>
                </div>
              )}
              <div className="template-detail__meta">
                <span>{t('scenarios.nodes', { count: selected.definition.nodes.length })}</span>
                <span>·</span>
                <span>{t('scenarios.edges', { count: selected.definition.edges.length })}</span>
              </div>
              <span className="eyebrow">{t('scenarios.steps')}</span>
              <ol className="step-list">
                {selected.definition.nodes.map((n) => {
                  const key = labelKeyForType(n.type_id)
                  const cat = visualCategoryForType(n.type_id)
                  return (
                    <li key={n.id}>
                      <span className={`palette__item-icon palette__item-icon--${cat}`}>
                        <IconForType typeId={n.type_id} size={12} />
                      </span>
                      <span className="step-list__label">{key ? t(key) : n.type_id}</span>
                      <code>{n.id}</code>
                    </li>
                  )
                })}
              </ol>
              <button type="button" className="btn btn--primary btn--block" onClick={() => use(selected)}>
                {t('scenarios.use')}
                <IconArrowRight size={14} />
              </button>
            </div>
          ) : (
            <EmptyState compact title={t('scenarios.pick')} />
          )}
        </aside>
      </div>
    </Modal>
  )
}
