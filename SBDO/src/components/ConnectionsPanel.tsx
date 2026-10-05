import { useEffect, useState, type ReactNode } from 'react'
import { createConnection, deleteConnection, getConnections, testConnection } from '../api/connections'
import type { Connection } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import { Modal, useConfirm } from '../ui/Modal'
import { Badge, Card, EmptyState, Field, Notice } from '../ui/primitives'
import { IconCheck, IconGlobe, IconKey, IconPlug, IconPlus, IconSend, IconSparkle, IconTrash, IconCode } from '../ui/icons'

interface Props {
  open: boolean
  onClose: () => void
}

type ConnType = 'http' | 'telegram' | 'openai' | 'github'

const TYPES: { id: ConnType; icon: ReactNode; key: MessageKey }[] = [
  { id: 'http', icon: <IconGlobe size={16} />, key: 'connections.type.http' },
  { id: 'telegram', icon: <IconSend size={16} />, key: 'connections.type.telegram' },
  { id: 'openai', icon: <IconSparkle size={16} />, key: 'connections.type.openai' },
  { id: 'github', icon: <IconCode size={16} />, key: 'connections.type.github' },
]

export function typeIcon(type: string, size = 15): ReactNode {
  if (type === 'telegram') return <IconSend size={size} />
  if (type === 'openai' || type === 'ai') return <IconSparkle size={size} />
  if (type === 'github') return <IconCode size={size} />
  return <IconGlobe size={size} />
}

const PLACEHOLDER: Record<ConnType, string> = {
  http: 'https://api.example.com',
  telegram: 'https://api.telegram.org',
  openai: 'https://api.openai.com/v1',
  github: 'https://api.github.com',
}

export function ConnectionsPanel({ open, onClose }: Props) {
  const { t } = usePreferences()
  const confirm = useConfirm()
  const [items, setItems] = useState<Connection[]>([])
  const [name, setName] = useState('')
  const [type, setType] = useState<ConnType>('http')
  const [baseUrl, setBaseUrl] = useState('')
  const [owner, setOwner] = useState('')
  const [repo, setRepo] = useState('')
  const [token, setToken] = useState('')
  const [message, setMessage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [testing, setTesting] = useState<string | null>(null)
  const [results, setResults] = useState<Record<string, { ok: boolean; message: string }>>({})

  async function refresh() {
    try {
      setItems(await getConnections())
    } catch (err) {
      setMessage(String(err))
    }
  }

  useEffect(() => {
    if (open) {
      setMessage(null)
      void refresh()
    }
  }, [open])

  function buildSecret() {
    if (!token) return undefined
    if (type === 'telegram') return { bot_token: token }
    if (type === 'openai') return { api_key: token }
    if (type === 'github') return { token }
    return { auth: { type: 'bearer', token } }
  }

  function buildConfig(): Record<string, unknown> {
    if (type === 'telegram') return baseUrl ? { api_base: baseUrl } : {}
    if (type === 'openai')
      return {
        base_url: baseUrl || 'https://api.openai.com/v1',
        default_model: 'gpt-4o-mini',
        models: [{ id: 'gpt-4o-mini', capabilities: { chat: true } }],
      }
    if (type === 'github')
      return {
        ...(owner.trim() ? { owner: owner.trim() } : {}),
        ...(repo.trim() ? { repo: repo.trim() } : {}),
        ...(baseUrl.trim() ? { api_base: baseUrl.trim() } : {}),
      }
    return baseUrl ? { base_url: baseUrl } : {}
  }

  async function submit() {
    setBusy(true)
    setMessage(null)
    try {
      await createConnection({ name, type, config: buildConfig(), secret: buildSecret(), enabled: true })
      setName('')
      setBaseUrl('')
      setOwner('')
      setRepo('')
      setToken('')
      await refresh()
      setMessage(t('connections.created'))
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function runTest(c: Connection) {
    setTesting(c.id)
    try {
      const r = await testConnection(c.id)
      setResults((prev) => ({ ...prev, [c.id]: r }))
    } catch (err) {
      setResults((prev) => ({ ...prev, [c.id]: { ok: false, message: String(err) } }))
    } finally {
      setTesting(null)
    }
  }

  async function remove(c: Connection) {
    const ok = await confirm({
      title: t('connections.delete'),
      message: t('connections.confirmDelete', { name: c.name }),
      confirmLabel: t('connections.delete'),
      danger: true,
    })
    if (!ok) return
    try {
      await deleteConnection(c.id)
      await refresh()
      setMessage(t('ingress.deleted'))
    } catch (err) {
      setMessage(String(err))
    }
  }

  const secretLabel =
    type === 'telegram'
      ? t('connections.botToken')
      : type === 'openai'
        ? t('connections.apiKey')
        : type === 'github'
          ? t('connections.githubToken')
          : t('connections.token')

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      flush
      icon={<IconPlug size={18} />}
      title={t('connections.title')}
      subtitle={t('connections.hint')}
    >
      <div className="split split--form">
        <aside className="split__side split__side--left">
          <Card title={t('connections.newTitle')}>
            <form
              className="form"
              onSubmit={(e) => {
                e.preventDefault()
                void submit()
              }}
            >
              <div className="type-picker">
                {TYPES.map((tp) => (
                  <button
                    key={tp.id}
                    type="button"
                    className={`type-picker__opt${type === tp.id ? ' is-active' : ''}`}
                    onClick={() => setType(tp.id)}
                  >
                    {tp.icon}
                    <span>{t(tp.key)}</span>
                  </button>
                ))}
              </div>
              <Field label={t('connections.name')}>
                <input value={name} onChange={(e) => setName(e.target.value)} required placeholder={t('connections.namePlaceholder')} />
              </Field>
              {type === 'github' && (
                <div className="form__row">
                  <Field label={t('connections.owner')}>
                    <input value={owner} onChange={(e) => setOwner(e.target.value)} placeholder="owner" />
                  </Field>
                  <Field label={t('connections.repo')}>
                    <input value={repo} onChange={(e) => setRepo(e.target.value)} placeholder="repo" />
                  </Field>
                </div>
              )}
              <Field label={type === 'telegram' || type === 'github' ? t('connections.apiBase') : t('connections.baseUrl')}>
                <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder={PLACEHOLDER[type]} spellCheck={false} />
              </Field>
              <Field
                label={secretLabel}
                hint={type === 'openai' ? t('connections.apiKeyHint') : type === 'github' ? t('connections.githubTokenHint') : t('connections.tokenHint')}
              >
                <input type="password" value={token} onChange={(e) => setToken(e.target.value)} autoComplete="off" />
              </Field>
              <div className="callout callout--muted">
                <IconKey size={14} />
                <span>{t('connections.secretNote')}</span>
              </div>
              <button type="submit" className="btn btn--primary btn--block" disabled={busy || !name.trim()}>
                <IconPlus size={14} />
                {t('connections.add')}
              </button>
            </form>
          </Card>
        </aside>

        <div className="split__main">
          <Notice text={message} onDismiss={() => setMessage(null)} />
          <h3 className="section-title">
            {t('connections.listTitle')} <span className="section-title__count">{items.length}</span>
          </h3>
          {items.length === 0 ? (
            <EmptyState icon={<IconPlug size={22} />} title={t('connections.empty')} hint={t('connections.emptyHint')} />
          ) : (
            <ul className="row-list">
              {items.map((c) => {
                const r = results[c.id]
                return (
                  <li key={c.id} className="row">
                    <span className={`row__icon row__icon--${c.type}`}>{typeIcon(c.type)}</span>
                    <div className="row__main">
                      <div className="row__title">
                        <strong>{c.name}</strong>
                        <span className="chip chip--mono">{c.type}</span>
                        {c.has_secret && (
                          <Badge tone="success">
                            <IconKey size={11} />
                            {t('connections.secret')}
                          </Badge>
                        )}
                        {!c.enabled && <Badge>{t('connections.off')}</Badge>}
                      </div>
                      <div className="row__meta">
                        <span className="mono">{String(c.config?.base_url ?? c.config?.api_base ?? c.id.slice(0, 8))}</span>
                        {r && (
                          <span className={r.ok ? 'tone-success' : 'tone-error'}>
                            {r.ok ? <IconCheck size={12} /> : '✕'} {r.message}
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="row__actions">
                      <button type="button" className="btn btn--sm" disabled={testing === c.id} onClick={() => void runTest(c)}>
                        {testing === c.id ? <span className="spinner spinner--xs" /> : null}
                        {t('connections.test')}
                      </button>
                      <button type="button" className="icon-btn icon-btn--danger" onClick={() => void remove(c)} aria-label={t('connections.delete')} data-tip={t('connections.delete')}>
                        <IconTrash size={14} />
                      </button>
                    </div>
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      </div>
    </Modal>
  )
}
