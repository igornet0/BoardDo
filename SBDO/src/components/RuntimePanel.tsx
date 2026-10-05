import { useEffect, useState } from 'react'
import { listRuntimes, reloadRuntime, startRuntime, stopRuntime } from '../api/runtime'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import type { RuntimeSnapshot, RuntimeState } from '../types'
import { Modal } from '../ui/Modal'
import { Badge, EmptyState, Notice, Stat, type Tone } from '../ui/primitives'
import { formatRelative, formatTime } from '../ui/format'
import {
  IconActivity,
  IconClock,
  IconExternal,
  IconLightning,
  IconPlay,
  IconRefresh,
  IconStop,
} from '../ui/icons'

interface Props {
  open: boolean
  currentId: string | null
  currentName: string
  canStartCurrent: boolean
  currentArmed: boolean
  onClose: () => void
  onOpen: (id: string) => void
  onCurrentStart: () => void
  onCurrentStop: () => void
  onCurrentReload: () => void
  onChanged?: () => void
}

function stateKey(state: RuntimeState): MessageKey {
  return `runtime.state.${state}` as MessageKey
}

function stateTone(state: RuntimeState): Tone {
  if (state === 'executing') return 'warning'
  if (state === 'waiting' || state === 'running') return 'success'
  if (state === 'starting' || state === 'stopping') return 'info'
  return 'neutral'
}

function isArmed(state: RuntimeState) {
  return state !== 'stopped' && state !== 'stopping'
}

export function RuntimePanel({
  open,
  currentId,
  currentName,
  canStartCurrent,
  currentArmed,
  onClose,
  onOpen,
  onCurrentStart,
  onCurrentStop,
  onCurrentReload,
  onChanged,
}: Props) {
  const { t, locale } = usePreferences()
  const [items, setItems] = useState<RuntimeSnapshot[]>([])
  const [loading, setLoading] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)

  async function refresh() {
    setLoading(true)
    try {
      setItems(await listRuntimes())
    } catch (err) {
      setMessage(String(err))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (!open) return
    setMessage(null)
    void refresh()
    const timer = window.setInterval(() => void refresh(), 2500)
    return () => window.clearInterval(timer)
  }, [open])

  async function act(id: string, fn: () => Promise<string>) {
    setBusyId(id)
    setMessage(null)
    try {
      setMessage(await fn())
      await refresh()
      onChanged?.()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusyId(null)
    }
  }

  const executing = items.filter((i) => i.state === 'executing').length
  const waiting = items.filter((i) => i.state === 'waiting' || i.state === 'running').length
  const totalRuns = items.reduce((sum, i) => sum + i.executions_total, 0)

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      icon={<IconActivity size={18} />}
      title={t('runtime.panel.title')}
      subtitle={t('runtime.panel.hint')}
      actions={
        <button type="button" className="icon-btn" onClick={() => void refresh()} data-tip={t('runtime.panel.refresh')} aria-label={t('runtime.panel.refresh')}>
          <IconRefresh size={15} className={loading ? 'spin' : undefined} />
        </button>
      }
    >
      <div className="stat-grid">
        <Stat icon={<IconActivity size={14} />} label={t('runtime.panel.stat.live')} value={items.length} tone="info" />
        <Stat icon={<IconClock size={14} />} label={t('runtime.panel.stat.waiting')} value={waiting} tone="success" />
        <Stat icon={<IconLightning size={14} />} label={t('runtime.panel.stat.executing')} value={executing} tone="warning" />
        <Stat icon={<IconPlay size={14} />} label={t('runtime.panel.stat.runs')} value={totalRuns} />
      </div>

      <div className={`current-board${currentArmed ? ' is-armed' : ''}`}>
        <div className="current-board__info">
          <span className="eyebrow">{t('runtime.panel.current')}</span>
          <div className="current-board__title">
            <span className={`runtime-dot runtime-dot--${currentArmed ? 'waiting' : 'stopped'}`} />
            <strong>{currentName || t('runtime.panel.untitled')}</strong>
            <Badge tone={currentArmed ? 'success' : 'neutral'}>
              {currentArmed ? t('runtime.panel.armed') : t('runtime.panel.idle')}
            </Badge>
          </div>
          <p>{currentArmed ? t('runtime.panel.armedHint') : t('runtime.panel.idleHint')}</p>
        </div>
        <div className="current-board__actions">
          {currentArmed ? (
            <>
              <button type="button" className="btn" disabled={busyId !== null} onClick={onCurrentReload} data-tip={t('runtime.reloadHint')}>
                <IconRefresh size={14} />
                {t('runtime.reload')}
              </button>
              <button type="button" className="btn btn--danger" disabled={busyId !== null} onClick={onCurrentStop}>
                <IconStop size={12} />
                {t('runtime.stop')}
              </button>
            </>
          ) : (
            <button type="button" className="btn btn--primary" disabled={busyId !== null || !canStartCurrent} onClick={onCurrentStart}>
              <IconPlay size={13} />
              {t('runtime.start')}
            </button>
          )}
        </div>
      </div>

      <Notice text={message} onDismiss={() => setMessage(null)} />

      <h3 className="section-title">{t('runtime.panel.list')}</h3>
      {loading && items.length === 0 ? (
        <div className="skeleton skeleton--row" />
      ) : items.length === 0 ? (
        <EmptyState
          compact
          icon={<IconActivity size={20} />}
          title={t('runtime.panel.empty')}
          hint={t('runtime.panel.emptyHint')}
        />
      ) : (
        <ul className="row-list">
          {items.map((item) => {
            const armed = isArmed(item.state)
            const isCurrent = item.workflow_id === currentId
            const busy = busyId === item.workflow_id
            const trigger = item.triggers[0]
            return (
              <li key={item.workflow_id} className={`row${isCurrent ? ' is-current' : ''}`}>
                <span className={`runtime-dot runtime-dot--${item.state}`} aria-hidden />
                <div className="row__main">
                  <div className="row__title">
                    <strong>{item.workflow_name}</strong>
                    <Badge tone={stateTone(item.state)}>{t(stateKey(item.state))}</Badge>
                    {isCurrent && <Badge tone="info">{t('runtime.panel.thisBoard')}</Badge>}
                  </div>
                  <div className="row__meta">
                    <span>
                      <IconLightning size={12} />
                      {trigger
                        ? `${trigger.label}${trigger.account_hint ? ` · ${trigger.account_hint}` : ''}`
                        : t('runtime.panel.noTrigger')}
                    </span>
                    <span>{t('runtime.executions', { count: item.executions_total })}</span>
                    {item.in_flight > 0 && <span>{t('runtime.panel.inFlight', { count: item.in_flight })}</span>}
                    <span title={formatTime(item.last_event_at, locale)}>
                      {t('runtime.lastEvent')}: {item.last_event_at ? formatRelative(item.last_event_at, locale) : '—'}
                    </span>
                    {item.next_run_at && (
                      <span title={formatTime(item.next_run_at, locale)}>
                        {t('runtime.nextRun')}: {formatRelative(item.next_run_at, locale)}
                      </span>
                    )}
                  </div>
                </div>
                <div className="row__actions">
                  <button
                    type="button"
                    className="icon-btn"
                    disabled={busy}
                    data-tip={t('runtime.panel.open')}
                    aria-label={t('runtime.panel.open')}
                    onClick={() => {
                      onOpen(item.workflow_id)
                      onClose()
                    }}
                  >
                    <IconExternal size={14} />
                  </button>
                  {armed ? (
                    <>
                      <button
                        type="button"
                        className="icon-btn"
                        disabled={busy}
                        data-tip={t('runtime.reload')}
                        aria-label={t('runtime.reload')}
                        onClick={() =>
                          void act(item.workflow_id, async () => {
                            const snap = await reloadRuntime(item.workflow_id)
                            return t('runtime.reloaded', { name: snap.workflow_name })
                          })
                        }
                      >
                        <IconRefresh size={14} />
                      </button>
                      <button
                        type="button"
                        className="btn btn--danger btn--sm"
                        disabled={busy}
                        onClick={() =>
                          void act(item.workflow_id, async () => {
                            await stopRuntime(item.workflow_id)
                            return t('runtime.stopped')
                          })
                        }
                      >
                        <IconStop size={11} />
                        {t('runtime.stop')}
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="btn btn--primary btn--sm"
                      disabled={busy}
                      onClick={() =>
                        void act(item.workflow_id, async () => {
                          const snap = await startRuntime(item.workflow_id)
                          return t('runtime.started', { name: snap.workflow_name })
                        })
                      }
                    >
                      <IconPlay size={12} />
                      {t('runtime.start')}
                    </button>
                  )}
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </Modal>
  )
}
