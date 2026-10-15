import { useCallback, useEffect, useState } from 'react'
import { alertwim, type AlertWimStatus } from '../api/alertwim'
import { usePreferences } from '../settings/PreferencesContext'
import { Badge, Card, Notice } from '../ui/primitives'
import { IconExternal, IconPlay, IconRefresh, IconSend } from '../ui/icons'

/** AlertWim extension: install from GitHub, open the window, send a test alert. */
export function AlertWimSection({ active }: { active: boolean }) {
  const { t } = usePreferences()
  const [status, setStatus] = useState<AlertWimStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    try {
      setStatus(await alertwim.status())
    } catch (err) {
      setMsg(String(err))
    }
  }, [])

  useEffect(() => {
    if (!active) return
    void refresh()
    // Poll faster while an install runs.
    const id = window.setInterval(() => void refresh(), status?.install_job.running ? 2000 : 10_000)
    return () => window.clearInterval(id)
  }, [active, refresh, status?.install_job.running])

  const act = async (fn: () => Promise<unknown>, done?: string) => {
    setBusy(true)
    setMsg(null)
    try {
      await fn()
      if (done) setMsg(done)
    } catch (err) {
      setMsg(String(err))
    } finally {
      setBusy(false)
      void refresh()
    }
  }

  const job = status?.install_job
  const installing = Boolean(job?.running)
  const badge = !status ? null : status.running ? (
    <Badge tone="success" dot>
      {t('alertwim.running')}
      {status.version ? ` · v${status.version}` : ''}
    </Badge>
  ) : status.installed ? (
    <Badge tone="neutral" dot>
      {t('alertwim.stopped')}
    </Badge>
  ) : (
    <Badge tone="warning" dot>
      {t('alertwim.notInstalled')}
    </Badge>
  )

  return (
    <>
      <Notice text={msg} onDismiss={() => setMsg(null)} />
      <Card title={t('alertwim.title')} subtitle={t('alertwim.subtitle')} actions={badge}>
        {status && !status.installed && !installing && (
          <p className="editor__hint" style={{ marginTop: 0 }}>
            {t('alertwim.installHint')}{' '}
            <a href={status.repo.replace(/\.git$/, '')} target="_blank" rel="noreferrer">
              {status.repo.replace(/^https:\/\//, '').replace(/\.git$/, '')} <IconExternal size={11} />
            </a>
          </p>
        )}

        {status?.installed && (
          <dl className="kv" style={{ display: 'grid', gridTemplateColumns: 'max-content 1fr', gap: '4px 14px', margin: '0 0 12px' }}>
            <dt>{t('alertwim.binary')}</dt>
            <dd style={{ margin: 0, wordBreak: 'break-all' }}>
              <code>{status.binary ?? '—'}</code>
            </dd>
            {status.url && (
              <>
                <dt>{t('alertwim.url')}</dt>
                <dd style={{ margin: 0 }}>
                  <code>{status.url}</code>
                </dd>
              </>
            )}
            <dt>{t('alertwim.connection')}</dt>
            <dd style={{ margin: 0 }}>{status.connection_id ? `AlertWim · ${t('alertwim.connectionAuto')}` : '—'}</dd>
          </dl>
        )}

        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          {status && !status.installed && (
            <button type="button" className="btn btn--primary" disabled={busy || installing} onClick={() => act(alertwim.install)}>
              {installing ? t('alertwim.installing') : t('alertwim.install')}
            </button>
          )}
          {status?.installed && (
            <>
              <button type="button" className="btn btn--primary" disabled={busy} onClick={() => act(alertwim.launch)}>
                <IconPlay size={13} /> {t('alertwim.launch')}
              </button>
              <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => act(() => alertwim.test(), t('alertwim.testSent'))}>
                <IconSend size={13} /> {t('alertwim.test')}
              </button>
            </>
          )}
          <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => void refresh()}>
            <IconRefresh size={13} /> {t('alertwim.refresh')}
          </button>
        </div>

        {job && (job.running || job.error || job.log.length > 0) && (
          <div style={{ marginTop: 12 }}>
            <div className="editor__hint">
              {job.phase}
              {job.error ? ` — ${job.error}` : ''}
            </div>
            <pre
              style={{
                maxHeight: 180,
                overflow: 'auto',
                fontSize: 11,
                margin: '6px 0 0',
                padding: 8,
                borderRadius: 8,
                background: 'var(--surface-2, rgba(127,127,127,.08))',
              }}
            >
              {job.log.slice(-40).join('\n')}
            </pre>
          </div>
        )}
      </Card>
    </>
  )
}
