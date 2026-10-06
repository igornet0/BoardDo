import { useEffect, useState } from 'react'
import {
  connectTelegramAccount,
  createTelegramAccount,
  deleteTelegramAccount,
  disconnectTelegramAccount,
  getTelegramCapabilities,
  listTelegramAccounts,
  listTelegramChats,
  listTelegramMessages,
  searchTelegramChats,
  sendTelegramUserMessage,
  simulateTelegramMessage,
  submitTelegramCode,
  submitTelegramPassword,
  submitTelegramPhone,
  subscribeTelegramSocket,
  type TelegramAccount,
  type TelegramAccountStatus,
  type TelegramChat,
  type TelegramMessage,
} from '../api/telegram'
import { usePreferences } from '../settings/PreferencesContext'
import { useConfirm } from '../ui/Modal'
import { Avatar, Badge, Card, EmptyState, Field, Notice, SearchInput, type Tone } from '../ui/primitives'
import { IconCopy, IconFlask, IconPlus, IconRefresh, IconSend, IconTrash, IconUser, IconClose } from '../ui/icons'

interface Props {
  active: boolean
}

function statusLabel(status: TelegramAccountStatus): string {
  return status.replaceAll('_', ' ')
}

function statusTone(status: TelegramAccountStatus): Tone {
  if (status === 'ready') return 'success'
  if (status === 'error') return 'error'
  if (status === 'disconnected') return 'neutral'
  if (needsAuthInput(status)) return 'warning'
  return 'info'
}

function needsAuthInput(status: TelegramAccountStatus): boolean {
  return (
    status === 'wait_phone_number' ||
    status === 'wait_code' ||
    status === 'wait_password' ||
    status === 'wait_registration'
  )
}

function chatLabel(chat: TelegramChat): string {
  if (chat.username) return `${chat.title} (@${chat.username})`
  return chat.title || String(chat.id)
}

export function TelegramAccountsSection({ active }: Props) {
  const { t } = usePreferences()
  const confirm = useConfirm()
  const [items, setItems] = useState<TelegramAccount[]>([])
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [consent, setConsent] = useState<string | null>(null)
  const [authId, setAuthId] = useState<string | null>(null)
  const [phone, setPhone] = useState('')
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [workspaceId, setWorkspaceId] = useState<string | null>(null)
  const [chats, setChats] = useState<TelegramChat[]>([])
  const [selectedChatId, setSelectedChatId] = useState<number | null>(null)
  const [messages, setMessages] = useState<TelegramMessage[]>([])
  const [searchQuery, setSearchQuery] = useState('')
  const [searchResults, setSearchResults] = useState<TelegramChat[]>([])
  const [copiedChatId, setCopiedChatId] = useState<number | null>(null)
  const [draft, setDraft] = useState('')
  const [mockSimulate, setMockSimulate] = useState(false)
  const [simText, setSimText] = useState('BTC LONG')

  async function refresh() {
    try {
      setItems(await listTelegramAccounts())
    } catch (err) {
      setMessage(String(err))
    }
  }

  async function loadChats(accountId: string) {
    const res = await listTelegramChats(accountId)
    setChats(res.chats)
  }

  async function openWorkspace(accountId: string) {
    setWorkspaceId(accountId)
    setSelectedChatId(null)
    setMessages([])
    setSearchResults([])
    setSearchQuery('')
    setBusy(true)
    setMessage(null)
    try {
      await loadChats(accountId)
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function openChat(accountId: string, chatId: number) {
    setSelectedChatId(chatId)
    setBusy(true)
    setMessage(null)
    try {
      setMessages(await listTelegramMessages(accountId, chatId))
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    if (!active) return
    void refresh()
    void getTelegramCapabilities()
      .then((caps) => setMockSimulate(caps.mock_simulate))
      .catch(() => setMockSimulate(false))
    const unsubscribe = subscribeTelegramSocket((event) => {
      if (event.type !== 'auth_state_changed' || typeof event.account_id !== 'string') {
        return
      }
      const accountId = event.account_id
      const state = event.state as TelegramAccountStatus | undefined
      const error = typeof event.error === 'string' ? event.error : undefined
      if (!state) {
        void refresh()
        return
      }
      setItems((prev) => {
        const exists = prev.some((account) => account.id === accountId)
        if (!exists) {
          void refresh()
          return prev
        }
        return prev.map((account) =>
          account.id === accountId
            ? { ...account, status: state, error: error ?? null }
            : account,
        )
      })
      if (state === 'error' && error) {
        setMessage(error)
      }
    })
    return unsubscribe
  }, [active])

  if (!active) return null

  const authAccount = items.find((a) => a.id === authId) ?? null
  const workspace = items.find((a) => a.id === workspaceId) ?? null
  const selectedChat =
    chats.find((c) => c.id === selectedChatId) ??
    searchResults.find((c) => c.id === selectedChatId) ??
    null

  async function addAccount() {
    setBusy(true)
    setMessage(null)
    try {
      const created = await createTelegramAccount()
      setConsent(created.consent)
      setAuthId(created.account.id)
      setItems((prev) => {
        const rest = prev.filter((a) => a.id !== created.account.id)
        return [created.account, ...rest]
      })
      if (created.account.status === 'error' && created.account.error) {
        setMessage(created.account.error)
      }
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function removeAccount(account: TelegramAccount) {
    const ok = await confirm({
      title: t('telegram.delete'),
      message: t('telegram.confirmDelete'),
      confirmLabel: t('telegram.delete'),
      danger: true,
    })
    if (!ok) return
    try {
      await deleteTelegramAccount(account.id)
      if (authId === account.id) setAuthId(null)
      if (workspaceId === account.id) {
        setWorkspaceId(null)
        setChats([])
        setMessages([])
      }
      await refresh()
    } catch (err) {
      setMessage(String(err))
    }
  }

  const authSteps: TelegramAccountStatus[] = ['wait_phone_number', 'wait_code', 'wait_password']
  const authStepIndex = authAccount ? authSteps.indexOf(authAccount.status) : -1

  return (
    <Card
      title={t('settings.telegram.accounts')}
      subtitle={t('telegram.hint')}
      actions={
        <button type="button" className="btn btn--primary btn--sm" disabled={busy} onClick={() => void addAccount()}>
          <IconPlus size={13} />
          {t('telegram.add')}
        </button>
      }
    >
      {consent && <Notice tone="info" text={consent} onDismiss={() => setConsent(null)} />}
      <Notice text={message} onDismiss={() => setMessage(null)} />

      {authAccount && (authAccount.status === 'created' || authAccount.status === 'initializing') && (
        <div className="callout callout--info">
          <span className="spinner spinner--xs" />
          <span>{t('telegram.connecting')}</span>
        </div>
      )}

      {authAccount && needsAuthInput(authAccount.status) && (
        <form
          className="auth-card"
          onSubmit={(e) => {
            e.preventDefault()
            void (async () => {
              if (!authId) return
              setBusy(true)
              setMessage(null)
              try {
                if (authAccount.status === 'wait_phone_number') {
                  await submitTelegramPhone(authId, phone)
                  setPhone('')
                } else if (authAccount.status === 'wait_code') {
                  await submitTelegramCode(authId, code)
                  setCode('')
                } else if (authAccount.status === 'wait_password') {
                  await submitTelegramPassword(authId, password)
                  setPassword('')
                }
                await refresh()
              } catch (err) {
                setMessage(String(err))
              } finally {
                setBusy(false)
              }
            })()
          }}
        >
          <div className="auth-card__head">
            <strong>{t('telegram.auth.step')}</strong>
            <button type="button" className="icon-btn icon-btn--sm" onClick={() => setAuthId(null)} aria-label={t('common.close')}>
              <IconClose size={13} />
            </button>
          </div>
          <ol className="stepper">
            {[t('telegram.phone'), t('telegram.code'), t('telegram.password')].map((label, i) => (
              <li key={label} className={i < authStepIndex ? 'is-done' : i === authStepIndex ? 'is-active' : ''}>
                <span>{i + 1}</span>
                {label}
              </li>
            ))}
          </ol>
          {authAccount.status === 'wait_phone_number' && (
            <Field label={t('telegram.phone')}>
              <input value={phone} onChange={(e) => setPhone(e.target.value)} placeholder="+358..." autoComplete="off" required autoFocus />
            </Field>
          )}
          {authAccount.status === 'wait_code' && (
            <Field label={t('telegram.code')}>
              <input className="mono" value={code} onChange={(e) => setCode(e.target.value)} autoComplete="one-time-code" required autoFocus />
            </Field>
          )}
          {authAccount.status === 'wait_password' && (
            <Field label={t('telegram.password')}>
              <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" required autoFocus />
            </Field>
          )}
          <div className="form__actions">
            <div className="toolbar__spacer" />
            <button type="submit" className="btn btn--primary" disabled={busy}>
              {t('telegram.continue')}
            </button>
          </div>
        </form>
      )}

      {items.length === 0 ? (
        <EmptyState compact icon={<IconUser size={18} />} title={t('telegram.noAccounts')} hint={t('telegram.noAccountsHint')} />
      ) : (
        <ul className="row-list">
          {items.map((account) => {
            const title = account.display_name || account.username || account.id.slice(0, 8)
            return (
              <li key={account.id} className={`row${workspaceId === account.id ? ' is-current' : ''}`}>
                <Avatar text={title} size={34} />
                <div className="row__main">
                  <div className="row__title">
                    <strong>{title}</strong>
                    {account.username && <span className="chip">@{account.username}</span>}
                    <Badge tone={statusTone(account.status)} dot>
                      {statusLabel(account.status)}
                    </Badge>
                  </div>
                  <div className="row__meta">
                    <span className="mono">{account.phone_masked || '—'}</span>
                    {account.error && <span className="tone-error">{account.error}</span>}
                  </div>
                </div>
                <div className="row__actions">
                  {account.status !== 'ready' && account.status !== 'disconnected' && (
                    <button type="button" className="btn btn--sm" onClick={() => setAuthId(account.id)}>
                      {t('telegram.continue')}
                    </button>
                  )}
                  {account.status === 'ready' && (
                    <>
                      <button type="button" className="btn btn--primary btn--sm" onClick={() => void openWorkspace(account.id)}>
                        {t('telegram.open')}
                      </button>
                      {mockSimulate && (
                        <button
                          type="button"
                          className="icon-btn"
                          data-tip={t('telegram.simulate')}
                          aria-label={t('telegram.simulate')}
                          onClick={() => {
                            void (async () => {
                              try {
                                await simulateTelegramMessage(account.id, 1, Date.now() % 1_000_000, simText || 'BTC LONG')
                                setMessage(t('telegram.simulated'))
                              } catch (err) {
                                setMessage(String(err))
                              }
                            })()
                          }}
                        >
                          <IconFlask size={14} />
                        </button>
                      )}
                      <button
                        type="button"
                        className="btn btn--ghost btn--sm"
                        onClick={() => {
                          void disconnectTelegramAccount(account.id)
                            .then(refresh)
                            .catch((err) => setMessage(String(err)))
                        }}
                      >
                        {t('telegram.disconnect')}
                      </button>
                    </>
                  )}
                  {account.status === 'disconnected' && (
                    <button
                      type="button"
                      className="btn btn--sm"
                      onClick={() => {
                        void connectTelegramAccount(account.id)
                          .then(refresh)
                          .catch((err) => setMessage(String(err)))
                      }}
                    >
                      {t('telegram.connect')}
                    </button>
                  )}
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    aria-label={t('telegram.delete')}
                    data-tip={t('telegram.delete')}
                    onClick={() => void removeAccount(account)}
                  >
                    <IconTrash size={14} />
                  </button>
                </div>
              </li>
            )
          })}
        </ul>
      )}

      {mockSimulate && items.some((a) => a.status === 'ready') && (
        <Field label={t('telegram.simulateText')} hint={t('telegram.simulateHint')}>
          <input value={simText} onChange={(e) => setSimText(e.target.value)} />
        </Field>
      )}

      {workspace && workspace.status === 'ready' && (
        <section className="tg-workspace">
          <header className="tg-workspace__head">
            <Avatar text={workspace.display_name || workspace.username || '?'} size={26} />
            <strong>
              {t('telegram.workspace')} · {workspace.display_name || workspace.username || workspace.id.slice(0, 8)}
            </strong>
            <div className="toolbar__spacer" />
            <button
              type="button"
              className="icon-btn"
              disabled={busy}
              data-tip={t('telegram.refreshChats')}
              aria-label={t('telegram.refreshChats')}
              onClick={() => {
                if (!workspaceId) return
                void loadChats(workspaceId).catch((err) => setMessage(String(err)))
              }}
            >
              <IconRefresh size={14} />
            </button>
            <button
              type="button"
              className="icon-btn"
              aria-label={t('telegram.closeWorkspace')}
              data-tip={t('telegram.closeWorkspace')}
              onClick={() => {
                setWorkspaceId(null)
                setChats([])
                setMessages([])
                setSelectedChatId(null)
              }}
            >
              <IconClose size={14} />
            </button>
          </header>

          <div className="tg-workspace__grid">
            <div className="tg-chats">
              <form
                className="tg-chats__search"
                onSubmit={(e) => {
                  e.preventDefault()
                  if (!workspaceId || !searchQuery.trim()) return
                  void (async () => {
                    setBusy(true)
                    setMessage(null)
                    try {
                      const res = await searchTelegramChats(workspaceId, searchQuery.trim())
                      setSearchResults(res.chats)
                      if (res.chats.length === 0) setMessage(t('telegram.searchEmpty'))
                    } catch (err) {
                      setMessage(String(err))
                    } finally {
                      setBusy(false)
                    }
                  })()
                }}
              >
                <SearchInput
                  value={searchQuery}
                  onChange={(v) => {
                    setSearchQuery(v)
                    if (!v) setSearchResults([])
                  }}
                  placeholder={t('telegram.searchPlaceholder')}
                />
              </form>
              <ul className="tg-chats__list">
                {(searchResults.length > 0 ? searchResults : chats).map((chat) => (
                  <li key={chat.id}>
                    <button
                      type="button"
                      className={`tg-chat${selectedChatId === chat.id ? ' is-active' : ''}`}
                      onClick={() => {
                        if (!workspaceId) return
                        if (!chats.some((c) => c.id === chat.id)) setChats((prev) => [chat, ...prev])
                        void openChat(workspaceId, chat.id)
                      }}
                    >
                      <Avatar text={chat.title || String(chat.id)} size={30} />
                      <span className="tg-chat__text">
                        <strong>{chatLabel(chat)}</strong>
                        <span>
                          {chat.chat_type} · {chat.id}
                        </span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
              {chats.length === 0 && searchResults.length === 0 && <p className="tg-chats__empty">{t('telegram.noChats')}</p>}
            </div>

            <div className="tg-thread">
              {selectedChatId == null ? (
                <EmptyState compact icon={<IconSend size={18} />} title={t('telegram.pickChat')} />
              ) : (
                <>
                  <div className="tg-thread__head">
                    <span>{selectedChat ? chatLabel(selectedChat) : t('telegram.messages')}</span>
                    <button
                      type="button"
                      className="tg-thread__id"
                      data-tip={t('telegram.copyChatId')}
                      onClick={() => {
                        void navigator.clipboard?.writeText(String(selectedChatId))
                        setCopiedChatId(selectedChatId)
                        window.setTimeout(() => setCopiedChatId(null), 1200)
                      }}
                    >
                      <code>{selectedChatId}</code>
                      {copiedChatId === selectedChatId ? <span>✓</span> : <IconCopy size={11} />}
                    </button>
                  </div>
                  <ul className="tg-thread__messages">
                    {[...messages].reverse().map((msg) => (
                      <li key={msg.message_id} className="tg-msg">
                        <span className="tg-msg__meta">
                          #{msg.message_id}
                          {msg.sender_id != null ? ` · ${msg.sender_id}` : ''}
                        </span>
                        <span className="tg-msg__text">{msg.text || '—'}</span>
                      </li>
                    ))}
                  </ul>
                  <form
                    className="tg-thread__compose"
                    onSubmit={(e) => {
                      e.preventDefault()
                      if (!workspaceId || selectedChatId == null || !draft.trim()) return
                      void (async () => {
                        setBusy(true)
                        setMessage(null)
                        try {
                          await sendTelegramUserMessage(workspaceId, selectedChatId, draft.trim())
                          setDraft('')
                          setMessages(await listTelegramMessages(workspaceId, selectedChatId))
                        } catch (err) {
                          setMessage(String(err))
                        } finally {
                          setBusy(false)
                        }
                      })()
                    }}
                  >
                    <input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={t('telegram.compose')} />
                    <button type="submit" className="btn btn--primary" disabled={busy || !draft.trim()} aria-label={t('telegram.send')}>
                      <IconSend size={14} />
                    </button>
                  </form>
                </>
              )}
            </div>
          </div>
        </section>
      )}
    </Card>
  )
}
