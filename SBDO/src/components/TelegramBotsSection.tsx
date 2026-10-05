import { useEffect, useState } from 'react'
import { createConnection, deleteConnection, getConnections, testConnection } from '../api/connections'
import type { Connection } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import { useConfirm } from '../ui/Modal'
import { Badge, Card, EmptyState, Field, Notice } from '../ui/primitives'
import { IconBot, IconCheck, IconKey, IconPlus, IconTrash } from '../ui/icons'

interface Props {
  active: boolean
}

export function TelegramBotsSection({ active }: Props) {
  const { t } = usePreferences()
  const confirm = useConfirm()
  const [items, setItems] = useState<Connection[]>([])
  const [name, setName] = useState('')
  const [token, setToken] = useState('')
  const [apiBase, setApiBase] = useState('')
  const [message, setMessage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [showForm, setShowForm] = useState(false)
  const [results, setResults] = useState<Record<string, { ok: boolean; message: string }>>({})

  async function refresh() {
    try {
      const all = await getConnections()
      setItems(all.filter((c) => c.type === 'telegram'))
    } catch (err) {
      setMessage(String(err))
    }
  }

  useEffect(() => {
    if (active) void refresh()
  }, [active])

  if (!active) return null

  return (
    <Card
      title={t('settings.telegram.bots')}
      subtitle={t('settings.telegram.botsHint')}
      actions={
        !showForm && (
          <button type="button" className="btn btn--sm" onClick={() => setShowForm(true)}>
            <IconPlus size={13} />
            {t('settings.telegram.addBot')}
          </button>
        )
      }
    >
      <Notice text={message} onDismiss={() => setMessage(null)} />

      {showForm && (
        <form
          className="form form--inset"
          onSubmit={(e) => {
            e.preventDefault()
            void (async () => {
              setBusy(true)
              setMessage(null)
              try {
                await createConnection({
                  name,
                  type: 'telegram',
                  config: apiBase.trim() ? { api_base: apiBase.trim() } : {},
                  secret: token.trim() ? { bot_token: token.trim() } : undefined,
                  enabled: true,
                })
                setName('')
                setToken('')
                setApiBase('')
                setShowForm(false)
                await refresh()
                setMessage(t('settings.telegram.botCreated'))
              } catch (err) {
                setMessage(String(err))
              } finally {
                setBusy(false)
              }
            })()
          }}
        >
          <div className="form__row">
            <Field label={t('settings.telegram.botName')}>
              <input value={name} onChange={(e) => setName(e.target.value)} required autoFocus />
            </Field>
            <Field label={t('connections.apiBase')}>
              <input value={apiBase} onChange={(e) => setApiBase(e.target.value)} placeholder="https://api.telegram.org" />
            </Field>
          </div>
          <Field label={t('connections.botToken')} hint={t('connections.tokenHint')}>
            <input type="password" value={token} onChange={(e) => setToken(e.target.value)} autoComplete="off" required />
          </Field>
          <div className="form__actions">
            <div className="toolbar__spacer" />
            <button type="button" className="btn btn--ghost" onClick={() => setShowForm(false)}>
              {t('common.cancel')}
            </button>
            <button type="submit" className="btn btn--primary" disabled={busy}>
              {t('settings.telegram.addBot')}
            </button>
          </div>
        </form>
      )}

      {items.length === 0 && !showForm ? (
        <EmptyState compact icon={<IconBot size={18} />} title={t('settings.telegram.noBots')} />
      ) : (
        <ul className="row-list">
          {items.map((c) => {
            const r = results[c.id]
            return (
              <li key={c.id} className="row">
                <span className="row__icon row__icon--telegram">
                  <IconBot size={15} />
                </span>
                <div className="row__main">
                  <div className="row__title">
                    <strong>{c.name}</strong>
                    {c.has_secret && (
                      <Badge tone="success">
                        <IconKey size={11} />
                        {t('connections.secret')}
                      </Badge>
                    )}
                    {!c.enabled && <Badge>{t('connections.off')}</Badge>}
                  </div>
                  {r && (
                    <div className="row__meta">
                      <span className={r.ok ? 'tone-success' : 'tone-error'}>
                        {r.ok ? <IconCheck size={12} /> : '✕'} {r.message}
                      </span>
                    </div>
                  )}
                </div>
                <div className="row__actions">
                  <button
                    type="button"
                    className="btn btn--sm"
                    onClick={() =>
                      void testConnection(c.id)
                        .then((res) => setResults((p) => ({ ...p, [c.id]: res })))
                        .catch((err) => setResults((p) => ({ ...p, [c.id]: { ok: false, message: String(err) } })))
                    }
                  >
                    {t('connections.test')}
                  </button>
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    aria-label={t('connections.delete')}
                    data-tip={t('connections.delete')}
                    onClick={() =>
                      void (async () => {
                        const ok = await confirm({
                          title: t('connections.delete'),
                          message: t('settings.telegram.confirmDeleteBot'),
                          confirmLabel: t('connections.delete'),
                          danger: true,
                        })
                        if (!ok) return
                        try {
                          await deleteConnection(c.id)
                          await refresh()
                        } catch (err) {
                          setMessage(String(err))
                        }
                      })()
                    }
                  >
                    <IconTrash size={14} />
                  </button>
                </div>
              </li>
            )
          })}
        </ul>
      )}
    </Card>
  )
}
