import { useEffect, useMemo, useState } from 'react'
import { deleteWorkflow, getWorkflows } from '../api/workflows'
import { usePreferences } from '../settings/PreferencesContext'
import type { WorkflowStatus, WorkflowSummary } from '../types'
import type { MessageKey } from '../i18n/messages'
import { Modal, useConfirm } from '../ui/Modal'
import { Badge, EmptyState, Notice, SearchInput, Segmented, type Tone } from '../ui/primitives'
import { formatDateTime, formatRelative } from '../ui/format'
import { IconFolder, IconPlus, IconRefresh, IconTemplate, IconTrash } from '../ui/icons'

interface Props {
  open: boolean
  currentId: string | null
  onClose: () => void
  onOpen: (id: string) => void
  onNew: () => void
  onOpenScenarios?: () => void
}

type Filter = 'all' | WorkflowStatus

const STATUS_TONE: Record<WorkflowStatus, Tone> = {
  active: 'success',
  draft: 'neutral',
  archived: 'warning',
}

export function ProjectsPanel({ open, currentId, onClose, onOpen, onNew, onOpenScenarios }: Props) {
  const { t, locale } = usePreferences()
  const confirm = useConfirm()
  const [items, setItems] = useState<WorkflowSummary[]>([])
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')

  async function refresh() {
    setLoading(true)
    try {
      const list = await getWorkflows()
      setItems(
        [...list].sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime()),
      )
    } catch (err) {
      setMessage(String(err))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (open) {
      setMessage(null)
      void refresh()
    }
  }, [open])

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    return items.filter(
      (i) =>
        (filter === 'all' || i.status === filter) &&
        (!q || i.name.toLowerCase().includes(q) || i.description.toLowerCase().includes(q)),
    )
  }, [items, query, filter])

  async function remove(item: WorkflowSummary) {
    const ok = await confirm({
      title: t('projects.delete'),
      message: t('projects.confirmDelete', { name: item.name }),
      confirmLabel: t('projects.delete'),
      danger: true,
    })
    if (!ok) return
    setBusy(true)
    try {
      await deleteWorkflow(item.id)
      setMessage(t('projects.deleted'))
      await refresh()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  const counts = {
    all: items.length,
    active: items.filter((i) => i.status === 'active').length,
    draft: items.filter((i) => i.status === 'draft').length,
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      icon={<IconFolder size={18} />}
      title={t('projects.title')}
      subtitle={t('projects.hint')}
      actions={
        <button
          type="button"
          className="icon-btn"
          disabled={loading}
          onClick={() => void refresh()}
          data-tip={t('projects.refresh')}
          aria-label={t('projects.refresh')}
        >
          <IconRefresh size={15} className={loading ? 'spin' : undefined} />
        </button>
      }
    >
      <div className="toolbar">
        <SearchInput value={query} onChange={setQuery} placeholder={t('projects.search')} autoFocus />
        <Segmented<Filter>
          value={filter}
          onChange={setFilter}
          options={[
            { value: 'all', label: `${t('scenarios.filter.all')} · ${counts.all}` },
            { value: 'active', label: `${t('projects.status.active')} · ${counts.active}` },
            { value: 'draft', label: `${t('projects.status.draft')} · ${counts.draft}` },
          ]}
        />
        <div className="toolbar__spacer" />
        {onOpenScenarios && (
          <button type="button" className="btn" onClick={onOpenScenarios}>
            <IconTemplate size={14} />
            {t('projects.fromScenario')}
          </button>
        )}
        <button
          type="button"
          className="btn btn--primary"
          onClick={() => {
            onNew()
            onClose()
          }}
        >
          <IconPlus size={14} />
          {t('projects.new')}
        </button>
      </div>

      <Notice text={message} onDismiss={() => setMessage(null)} />

      {loading && items.length === 0 ? (
        <div className="card-grid">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="skeleton skeleton--card" />
          ))}
        </div>
      ) : items.length === 0 ? (
        <EmptyState
          icon={<IconFolder size={22} />}
          title={t('projects.empty')}
          hint={t('projects.emptyHint')}
          action={
            onOpenScenarios && (
              <button type="button" className="btn btn--primary" onClick={onOpenScenarios}>
                <IconTemplate size={14} />
                {t('projects.fromScenario')}
              </button>
            )
          }
        />
      ) : visible.length === 0 ? (
        <EmptyState compact title={t('projects.noMatch')} />
      ) : (
        <div className="card-grid">
          {visible.map((item) => {
            const isCurrent = item.id === currentId
            return (
              <article
                key={item.id}
                className={`project-card${isCurrent ? ' is-current' : ''}`}
                onClick={() => {
                  onOpen(item.id)
                  onClose()
                }}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    onOpen(item.id)
                    onClose()
                  }
                }}
              >
                <div className="project-card__head">
                  <span className="project-card__icon">{item.name.trim()[0]?.toUpperCase() ?? '?'}</span>
                  <div className="project-card__titles">
                    <strong>{item.name}</strong>
                    <span title={formatDateTime(item.updated_at, locale)}>
                      {t('projects.updated')} {formatRelative(item.updated_at, locale)}
                    </span>
                  </div>
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger project-card__delete"
                    disabled={busy}
                    aria-label={t('projects.delete')}
                    data-tip={t('projects.delete')}
                    onClick={(e) => {
                      e.stopPropagation()
                      void remove(item)
                    }}
                  >
                    <IconTrash size={14} />
                  </button>
                </div>
                <p className="project-card__desc">{item.description || t('projects.noDescription')}</p>
                <div className="project-card__foot">
                  <Badge tone={STATUS_TONE[item.status]} dot>
                    {t(`projects.status.${item.status}` as MessageKey)}
                  </Badge>
                  <span className="chip chip--mono">v{item.version}</span>
                  {isCurrent && <Badge tone="info">{t('projects.current')}</Badge>}
                </div>
              </article>
            )
          })}
        </div>
      )}
    </Modal>
  )
}
