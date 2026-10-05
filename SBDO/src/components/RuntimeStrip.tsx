import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import type { RuntimeSnapshot, RuntimeState } from '../types'

interface Props {
  snapshot: RuntimeSnapshot | null
  liveCount: number
  busy: boolean
  canStart: boolean
  onStart: () => void
  onStop: () => void
  onReload: () => void
  onManage: () => void
}

function stateKey(state: RuntimeState): MessageKey {
  return `runtime.state.${state}` as MessageKey
}

function formatTime(iso?: string | null) {
  if (!iso) return '—'
  try {
    return new Date(iso).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    })
  } catch {
    return iso
  }
}

export function RuntimeStrip({
  snapshot,
  liveCount,
  busy,
  canStart,
  onStart,
  onStop,
  onReload,
  onManage,
}: Props) {
  const { t } = usePreferences()
  const state = snapshot?.state ?? 'stopped'
  const armed = state !== 'stopped' && state !== 'stopping'
  const trigger = snapshot?.triggers[0]
  const account = trigger?.account_hint

  return (
    <div className={`runtime-strip${armed ? ' runtime-strip--armed' : ''}`}>
      <button
        type="button"
        className="runtime-strip__status"
        onClick={onManage}
        title={t('runtime.panel.title')}
      >
        <span className={`runtime-dot runtime-dot--${state}`} aria-hidden />
        <div className="runtime-strip__main">
          <strong>{t('runtime.title')}</strong>
          <span className="runtime-strip__state">{t(stateKey(state))}</span>
          {liveCount > 0 && (
            <span className="runtime-strip__badge">
              {t('runtime.liveCount', { count: liveCount })}
            </span>
          )}
          {trigger && (
            <span className="runtime-strip__trigger">
              {t('runtime.trigger')}: {trigger.label}
              {account ? ` · ${account}` : ''}
            </span>
          )}
          {snapshot && armed && (
            <span className="runtime-strip__meta">
              {t('runtime.executions', { count: snapshot.executions_total })}
              {' · '}
              {t('runtime.lastEvent')}: {formatTime(snapshot.last_event_at)}
              {snapshot.next_run_at
                ? ` · ${t('runtime.nextRun')}: ${formatTime(snapshot.next_run_at)}`
                : ''}
            </span>
          )}
        </div>
      </button>
      <div className="runtime-strip__actions">
        <button
          type="button"
          className="btn btn--ghost runtime-strip__manage"
          onClick={onManage}
        >
          {t('runtime.manage')}
        </button>
        {armed ? (
          <>
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={onReload}
              title={t('runtime.reloadHint')}
            >
              {t('runtime.reload')}
            </button>
            <button
              type="button"
              className="btn btn--danger"
              disabled={busy}
              onClick={onStop}
            >
              {t('runtime.stop')}
            </button>
          </>
        ) : (
          <button
            type="button"
            className="btn btn--primary"
            disabled={busy || !canStart}
            onClick={onStart}
          >
            {t('runtime.start')}
          </button>
        )}
      </div>
    </div>
  )
}
