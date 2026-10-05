import { useEffect, useState } from 'react'
import { usePreferences } from '../settings/PreferencesContext'
import type { Locale, MessageKey } from '../i18n/messages'
import type { ThemePreference } from '../settings/preferences'
import {
  getAgentSettings,
  testAgentSettings,
  updateAgentSettings,
  type AgentSettings,
} from '../api/agent'
import {
  createConnection,
  getConnections,
  updateConnection,
} from '../api/connections'
import type { Connection } from '../types'
import { TelegramAccountsSection } from './TelegramAccountsPanel'
import { TelegramBotsSection } from './TelegramBotsSection'
import { Modal } from '../ui/Modal'
import { Badge, Card, EmptyState, Field, Notice, Switch } from '../ui/primitives'
import {
  IconCheck,
  IconEdit,
  IconGlobe,
  IconMonitor,
  IconMoon,
  IconPalette,
  IconPlus,
  IconSend,
  IconSettings,
  IconSparkle,
  IconSun,
} from '../ui/icons'
import type { ReactNode } from 'react'

export type SettingsTab = 'theme' | 'agent' | 'telegram'

interface Props {
  open: boolean
  onClose: () => void
  tab?: SettingsTab
  onAgentSettingsChange?: (settings: AgentSettings) => void
}

const THEMES: { value: ThemePreference; key: MessageKey; icon: ReactNode }[] = [
  { value: 'light', key: 'settings.theme.light', icon: <IconSun size={14} /> },
  { value: 'dark', key: 'settings.theme.dark', icon: <IconMoon size={14} /> },
  { value: 'system', key: 'settings.theme.system', icon: <IconMonitor size={14} /> },
]

const LOCALES: { value: Locale; key: MessageKey; flag: string }[] = [
  { value: 'en', key: 'settings.language.en', flag: 'EN' },
  { value: 'ru', key: 'settings.language.ru', flag: 'RU' },
]

const TABS: { id: SettingsTab; key: MessageKey; hint: MessageKey; icon: ReactNode }[] = [
  { id: 'theme', key: 'settings.tab.theme', hint: 'settings.tab.themeHint', icon: <IconPalette size={16} /> },
  { id: 'agent', key: 'settings.tab.agent', hint: 'settings.tab.agentHint', icon: <IconSparkle size={16} /> },
  { id: 'telegram', key: 'settings.tab.telegram', hint: 'settings.tab.telegramHint', icon: <IconSend size={16} /> },
]

type CapKey =
  | 'chat'
  | 'image_generate'
  | 'image_edit'
  | 'video_generate'
  | 'audio_generate'
  | 'audio_transcribe'

const CAP_KEYS: CapKey[] = [
  'chat',
  'image_generate',
  'image_edit',
  'video_generate',
  'audio_generate',
  'audio_transcribe',
]

type ModelDraft = {
  id: string
  label: string
  capabilities: Record<CapKey, boolean>
}

function modelsFromConfig(config: Record<string, unknown> | undefined): ModelDraft[] {
  const raw = config?.models
  if (!Array.isArray(raw) || raw.length === 0) {
    const def = String(config?.default_model ?? 'gpt-4o-mini')
    return [
      {
        id: def,
        label: '',
        capabilities: {
          chat: true,
          image_generate: false,
          image_edit: false,
          video_generate: false,
          audio_generate: false,
          audio_transcribe: false,
        },
      },
    ]
  }
  return raw.map((item) => {
    const obj = (item ?? {}) as Record<string, unknown>
    const caps = (obj.capabilities ?? {}) as Record<string, unknown>
    return {
      id: String(obj.id ?? ''),
      label: String(obj.label ?? ''),
      capabilities: {
        chat: caps.chat !== false,
        image_generate: Boolean(caps.image_generate),
        image_edit: Boolean(caps.image_edit),
        video_generate: Boolean(caps.video_generate),
        audio_generate: Boolean(caps.audio_generate),
        audio_transcribe: Boolean(caps.audio_transcribe),
      },
    }
  })
}

function buildOpenaiConfig(
  baseUrl: string,
  defaultModel: string,
  models: ModelDraft[],
): Record<string, unknown> {
  return {
    base_url: baseUrl || 'https://api.openai.com/v1',
    default_model: defaultModel || models[0]?.id || 'gpt-4o-mini',
    models: models
      .filter((m) => m.id.trim())
      .map((m) => ({
        id: m.id.trim(),
        ...(m.label.trim() ? { label: m.label.trim() } : {}),
        capabilities: m.capabilities,
      })),
  }
}

export function SettingsPanel({
  open,
  onClose,
  tab: tabProp = 'theme',
  onAgentSettingsChange,
}: Props) {
  const { theme, locale, setTheme, setLocale, t } = usePreferences()
  const [tab, setTab] = useState<SettingsTab>(tabProp)
  const [agent, setAgent] = useState<AgentSettings | null>(null)
  const [connections, setConnections] = useState<Connection[]>([])
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)

  const [newName, setNewName] = useState('')
  const [newBaseUrl, setNewBaseUrl] = useState('https://api.openai.com/v1')
  const [newModel, setNewModel] = useState('gpt-4o-mini')
  const [newKey, setNewKey] = useState('')
  const [editId, setEditId] = useState<string | null>(null)
  const [editModels, setEditModels] = useState<ModelDraft[]>([])
  const [editBaseUrl, setEditBaseUrl] = useState('')
  const [editDefaultModel, setEditDefaultModel] = useState('')
  const [editKey, setEditKey] = useState('')

  useEffect(() => {
    if (!open) return
    setTab(tabProp)
  }, [open, tabProp])

  async function refresh() {
    const [s, list] = await Promise.all([getAgentSettings(), getConnections()])
    setAgent(s)
    onAgentSettingsChange?.(s)
    setConnections(list.filter((c) => c.type === 'openai' || c.type === 'ai'))
  }

  useEffect(() => {
    if (!open) return
    setMsg(null)
    void refresh().catch((err) => setMsg(String(err)))
  }, [open, onAgentSettingsChange])

  const openai = connections

  async function saveAgent() {
    if (!agent) return
    setBusy(true)
    setMsg(null)
    try {
      const next = await updateAgentSettings({
        enabled: agent.enabled,
        connection_id: agent.connection_id ?? null,
        model: agent.model ?? null,
      })
      setAgent(next)
      onAgentSettingsChange?.(next)
      setMsg(t('settings.agent.saved'))
    } catch (err) {
      setMsg(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function testAgent() {
    setBusy(true)
    setMsg(null)
    try {
      if (agent) {
        await updateAgentSettings({
          enabled: agent.enabled,
          connection_id: agent.connection_id ?? null,
          model: agent.model ?? null,
        })
      }
      const res = await testAgentSettings()
      setMsg(res.message)
    } catch (err) {
      setMsg(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function addProvider() {
    setBusy(true)
    setMsg(null)
    try {
      const models = modelsFromConfig({ default_model: newModel })
      await createConnection({
        name: newName.trim() || newBaseUrl || 'OpenAI',
        type: 'openai',
        config: buildOpenaiConfig(newBaseUrl, newModel, models),
        secret: newKey.trim() ? { api_key: newKey.trim() } : undefined,
        enabled: true,
      })
      setNewName('')
      setNewKey('')
      await refresh()
      setMsg(t('connections.created'))
    } catch (err) {
      setMsg(String(err))
    } finally {
      setBusy(false)
    }
  }

  function startEdit(c: Connection) {
    setEditId(c.id)
    setEditBaseUrl(String(c.config?.base_url ?? c.config?.api_base ?? ''))
    setEditDefaultModel(String(c.config?.default_model ?? 'gpt-4o-mini'))
    setEditModels(modelsFromConfig(c.config as Record<string, unknown>))
    setEditKey('')
  }

  async function saveEdit() {
    if (!editId) return
    const existing = openai.find((c) => c.id === editId)
    if (!existing) return
    setBusy(true)
    setMsg(null)
    try {
      await updateConnection(editId, {
        name: existing.name,
        type: existing.type,
        config: buildOpenaiConfig(editBaseUrl, editDefaultModel, editModels),
        secret: editKey.trim() ? { api_key: editKey.trim() } : undefined,
        enabled: existing.enabled,
      })
      setEditId(null)
      await refresh()
      setMsg(t('settings.agent.saved'))
    } catch (err) {
      setMsg(String(err))
    } finally {
      setBusy(false)
    }
  }

  const selectedModels = (() => {
    const c = openai.find((x) => x.id === agent?.connection_id)
    if (!c) return [] as ModelDraft[]
    return modelsFromConfig(c.config as Record<string, unknown>)
  })()

  const emptyCaps = {
    chat: true,
    image_generate: false,
    image_edit: false,
    video_generate: false,
    audio_generate: false,
    audio_transcribe: false,
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      flush
      icon={<IconSettings size={18} />}
      title={t('settings.title')}
      subtitle={t('settings.subtitle')}
    >
      <div className="settings">
        <nav className="settings__nav" aria-label={t('settings.title')}>
          {TABS.map(({ id, key, hint, icon }) => (
            <button
              key={id}
              type="button"
              className={`settings__nav-item${tab === id ? ' is-active' : ''}`}
              onClick={() => setTab(id)}
            >
              <span className="settings__nav-icon">{icon}</span>
              <span className="settings__nav-text">
                <strong>{t(key)}</strong>
                <span>{t(hint)}</span>
              </span>
            </button>
          ))}
        </nav>

        <div className="settings__content">
          {tab === 'theme' && (
            <>
              <Card title={t('settings.theme')} subtitle={t('settings.hint')}>
                <div className="theme-cards">
                  {THEMES.map(({ value, key, icon }) => (
                    <button
                      key={value}
                      type="button"
                      className={`theme-card${theme === value ? ' is-active' : ''}`}
                      onClick={() => setTheme(value)}
                    >
                      <span className={`theme-card__preview theme-card__preview--${value}`}>
                        <span className="theme-card__bar" />
                        <span className="theme-card__side" />
                        <span className="theme-card__node theme-card__node--a" />
                        <span className="theme-card__node theme-card__node--b" />
                      </span>
                      <span className="theme-card__label">
                        {icon}
                        {t(key)}
                        {theme === value && <IconCheck size={13} className="theme-card__check" />}
                      </span>
                    </button>
                  ))}
                </div>
              </Card>

              <Card title={t('settings.language')}>
                <div className="lang-cards">
                  {LOCALES.map(({ value, key, flag }) => (
                    <button
                      key={value}
                      type="button"
                      className={`lang-card${locale === value ? ' is-active' : ''}`}
                      onClick={() => setLocale(value)}
                    >
                      <span className="lang-card__flag">{flag}</span>
                      <span>{t(key)}</span>
                      {locale === value && <IconCheck size={14} className="lang-card__check" />}
                    </button>
                  ))}
                </div>
              </Card>
            </>
          )}

          {tab === 'agent' && (
            <>
              <Notice text={msg} onDismiss={() => setMsg(null)} />
              <Card
                title={t('settings.agent.assistant')}
                subtitle={t('settings.agent.unifiedHint')}
                actions={
                  agent?.has_connection ? (
                    <Badge tone="success" dot>
                      {t('settings.agent.connectionReady')}
                    </Badge>
                  ) : (
                    <Badge tone="warning" dot>
                      {t('settings.agent.notConfigured')}
                    </Badge>
                  )
                }
              >
                {agent ? (
                  <div className="form">
                    <Switch
                      checked={agent.enabled}
                      onChange={(v) => setAgent({ ...agent, enabled: v })}
                      label={t('settings.agent.enabled')}
                    />
                    <div className="form__row">
                      <Field label={t('settings.agent.connection')}>
                        <select
                          value={agent.connection_id ?? ''}
                          onChange={(e) => setAgent({ ...agent, connection_id: e.target.value || null, model: null })}
                        >
                          <option value="">{t('editor.none')}</option>
                          {openai.map((c) => (
                            <option key={c.id} value={c.id}>
                              {c.name}
                              {c.config?.default_model ? ` · ${String(c.config.default_model)}` : ''}
                            </option>
                          ))}
                        </select>
                      </Field>
                      <Field label={t('settings.agent.model')}>
                        {selectedModels.length > 0 ? (
                          <select value={agent.model ?? ''} onChange={(e) => setAgent({ ...agent, model: e.target.value || null })}>
                            <option value="">{agent.default_model ?? t('settings.agent.defaultModel')}</option>
                            {selectedModels.map((m) => (
                              <option key={m.id} value={m.id}>
                                {m.label || m.id}
                              </option>
                            ))}
                          </select>
                        ) : (
                          <input
                            value={agent.model ?? ''}
                            onChange={(e) => setAgent({ ...agent, model: e.target.value })}
                            spellCheck={false}
                            placeholder={agent.default_model ?? 'gpt-4o-mini'}
                          />
                        )}
                      </Field>
                    </div>
                    {agent.base_url && <p className="field__hint mono">{agent.base_url}</p>}
                    <div className="form__actions">
                      <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void saveAgent()}>
                        {t('settings.agent.save')}
                      </button>
                      <button type="button" className="btn" disabled={busy} onClick={() => void testAgent()}>
                        {t('settings.agent.test')}
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="skeleton skeleton--row" />
                )}
              </Card>

              <Card title={t('settings.agent.providers')} subtitle={t('settings.agent.providersHint')}>
                {openai.length === 0 ? (
                  <EmptyState compact icon={<IconSparkle size={18} />} title={t('settings.agent.noProviders')} />
                ) : (
                  <ul className="row-list">
                    {openai.map((c) => {
                      const inUse = agent?.connection_id === c.id
                      return (
                        <li key={c.id} className={`row row--stack${inUse ? ' is-current' : ''}`}>
                          <div className="row__line">
                            <span className="row__icon row__icon--openai">
                              <IconSparkle size={15} />
                            </span>
                            <div className="row__main">
                              <div className="row__title">
                                <strong>{c.name}</strong>
                                {c.config?.default_model ? <span className="chip chip--mono">{String(c.config.default_model)}</span> : null}
                                {c.has_secret && <Badge tone="success">{t('connections.secret')}</Badge>}
                                {inUse && <Badge tone="ai">{t('settings.agent.inUse')}</Badge>}
                              </div>
                              <div className="row__meta">
                                <span className="mono">
                                  <IconGlobe size={11} />
                                  {String(c.config?.base_url ?? c.config?.api_base ?? '')}
                                </span>
                              </div>
                            </div>
                            <div className="row__actions">
                              <button type="button" className="btn btn--sm" onClick={() => (editId === c.id ? setEditId(null) : startEdit(c))}>
                                <IconEdit size={12} />
                                {t('settings.agent.editModels')}
                              </button>
                              {!inUse && (
                                <button
                                  type="button"
                                  className="btn btn--sm"
                                  onClick={() => {
                                    if (!agent) return
                                    setAgent({ ...agent, connection_id: c.id, model: String(c.config?.default_model ?? '') || null })
                                  }}
                                >
                                  {t('settings.agent.useForChat')}
                                </button>
                              )}
                            </div>
                          </div>
                          {editId === c.id && (
                            <div className="provider-edit">
                              <div className="form__row">
                                <Field label={t('connections.baseUrl')}>
                                  <input value={editBaseUrl} onChange={(e) => setEditBaseUrl(e.target.value)} spellCheck={false} />
                                </Field>
                                <Field label={t('connections.defaultModel')}>
                                  <input value={editDefaultModel} onChange={(e) => setEditDefaultModel(e.target.value)} spellCheck={false} />
                                </Field>
                              </div>
                              <Field label={t('connections.apiKey')}>
                                <input
                                  type="password"
                                  value={editKey}
                                  onChange={(e) => setEditKey(e.target.value)}
                                  placeholder={c.has_secret ? t('settings.agent.apiKeySet') : t('settings.agent.apiKeyPlaceholder')}
                                  autoComplete="off"
                                />
                              </Field>
                              <span className="eyebrow">{t('settings.agent.models')}</span>
                              {editModels.map((m, idx) => (
                                <div key={idx} className="model-row">
                                  <input
                                    className="mono"
                                    value={m.id}
                                    onChange={(e) => {
                                      const next = [...editModels]
                                      next[idx] = { ...m, id: e.target.value }
                                      setEditModels(next)
                                    }}
                                    placeholder="model id"
                                  />
                                  <div className="cap-chips">
                                    {CAP_KEYS.map((key) => (
                                      <button
                                        key={key}
                                        type="button"
                                        className={`filter-chip filter-chip--sm${m.capabilities[key] ? ' is-active' : ''}`}
                                        onClick={() => {
                                          const next = [...editModels]
                                          next[idx] = { ...m, capabilities: { ...m.capabilities, [key]: !m.capabilities[key] } }
                                          setEditModels(next)
                                        }}
                                      >
                                        {t(`settings.cap.${key}` as MessageKey)}
                                      </button>
                                    ))}
                                  </div>
                                </div>
                              ))}
                              <div className="form__actions">
                                <button
                                  type="button"
                                  className="btn btn--sm"
                                  onClick={() => setEditModels([...editModels, { id: '', label: '', capabilities: { ...emptyCaps } }])}
                                >
                                  <IconPlus size={12} />
                                  {t('settings.agent.addModel')}
                                </button>
                                <div className="toolbar__spacer" />
                                <button type="button" className="btn btn--ghost btn--sm" onClick={() => setEditId(null)}>
                                  {t('common.cancel')}
                                </button>
                                <button type="button" className="btn btn--primary btn--sm" disabled={busy} onClick={() => void saveEdit()}>
                                  {t('app.save')}
                                </button>
                              </div>
                            </div>
                          )}
                        </li>
                      )
                    })}
                  </ul>
                )}
              </Card>

              <Card title={t('settings.agent.addProvider')}>
                <div className="form">
                  <div className="form__row">
                    <Field label={t('connections.name')}>
                      <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="OpenAI / DeepSeek" />
                    </Field>
                    <Field label={t('connections.defaultModel')}>
                      <input value={newModel} onChange={(e) => setNewModel(e.target.value)} spellCheck={false} />
                    </Field>
                  </div>
                  <Field label={t('connections.baseUrl')}>
                    <input value={newBaseUrl} onChange={(e) => setNewBaseUrl(e.target.value)} spellCheck={false} />
                  </Field>
                  <Field label={t('connections.apiKey')}>
                    <input type="password" value={newKey} onChange={(e) => setNewKey(e.target.value)} autoComplete="off" />
                  </Field>
                  <div className="form__actions">
                    <button type="button" className="btn btn--primary" disabled={busy || !newKey.trim()} onClick={() => void addProvider()}>
                      <IconPlus size={14} />
                      {t('connections.add')}
                    </button>
                  </div>
                </div>
              </Card>
            </>
          )}

          {tab === 'telegram' && (
            <>
              <TelegramBotsSection active={open && tab === 'telegram'} />
              <TelegramAccountsSection active={open && tab === 'telegram'} />
            </>
          )}
        </div>
      </div>
    </Modal>
  )
}
