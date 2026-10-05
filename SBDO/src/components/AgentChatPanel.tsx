import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { usePreferences } from '../settings/PreferencesContext'
import { agentChat, type AgentChatMessage, type AgentSettings } from '../api/agent'
import type { AgentWorkflowSnapshot } from '../api/agent'
import type { AgentGraphOp } from '../api/agent'
import { IconClose, IconSend, IconSparkle } from '../ui/icons'
import { labelKeyForType } from '../canvas/paletteItems'

interface Props {
  open: boolean
  onClose: () => void
  settings: AgentSettings | null
  workflow: AgentWorkflowSnapshot
  pinnedIds: string[]
  pinnedLabels: { id: string; typeId: string }[]
  selectMode: boolean
  onToggleSelectMode: () => void
  onRemovePin: (id: string) => void
  onClearPins: () => void
  onOps: (ops: AgentGraphOp[], summary: string) => void
  onOpenSettings: () => void
}

const STORAGE_KEY = 'boarddo.agent.widget'
const MIN_W = 320
const MIN_H = 360
const DEFAULT_W = 400
const DEFAULT_H = 520

type WidgetBox = { x: number; y: number; w: number; h: number }

function loadBox(): WidgetBox {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<WidgetBox>
      if (
        typeof parsed.x === 'number' &&
        typeof parsed.y === 'number' &&
        typeof parsed.w === 'number' &&
        typeof parsed.h === 'number'
      ) {
        return clampBox({
          x: parsed.x,
          y: parsed.y,
          w: parsed.w,
          h: parsed.h,
        })
      }
    }
  } catch {
    /* ignore */
  }
  return defaultBox()
}

function defaultBox(): WidgetBox {
  const w = DEFAULT_W
  const h = DEFAULT_H
  const margin = 24
  const fab = 72
  return {
    w,
    h,
    x: Math.max(margin, window.innerWidth - w - margin),
    y: Math.max(margin, window.innerHeight - h - fab - margin),
  }
}

function clampBox(box: WidgetBox): WidgetBox {
  const margin = 8
  const w = Math.min(Math.max(box.w, MIN_W), window.innerWidth - margin * 2)
  const h = Math.min(Math.max(box.h, MIN_H), window.innerHeight - margin * 2)
  const x = Math.min(Math.max(box.x, margin), window.innerWidth - w - margin)
  const y = Math.min(Math.max(box.y, margin), window.innerHeight - h - margin)
  return { x, y, w, h }
}

function saveBox(box: WidgetBox) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(box))
  } catch {
    /* ignore */
  }
}

export function AgentChatPanel({
  open,
  onClose,
  settings,
  workflow,
  pinnedIds,
  pinnedLabels,
  selectMode,
  onToggleSelectMode,
  onRemovePin,
  onClearPins,
  onOps,
  onOpenSettings,
}: Props) {
  const { t, locale } = usePreferences()
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [messages, setMessages] = useState<AgentChatMessage[]>([])
  const [box, setBox] = useState<WidgetBox>(() =>
    typeof window === 'undefined' ? { x: 0, y: 0, w: DEFAULT_W, h: DEFAULT_H } : loadBox(),
  )
  const scroller = useRef<HTMLDivElement>(null)
  const drag = useRef<{
    kind: 'move' | 'resize'
    startX: number
    startY: number
    origin: WidgetBox
  } | null>(null)
  const ready = Boolean(settings?.enabled && settings.has_connection)

  useEffect(() => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight })
  }, [messages, open])

  useEffect(() => {
    function onResize() {
      setBox((b) => {
        const next = clampBox(b)
        saveBox(next)
        return next
      })
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  function onPointerMove(ev: PointerEvent) {
    const d = drag.current
    if (!d) return
    const dx = ev.clientX - d.startX
    const dy = ev.clientY - d.startY
    if (d.kind === 'move') {
      const next = clampBox({
        ...d.origin,
        x: d.origin.x + dx,
        y: d.origin.y + dy,
      })
      setBox(next)
    } else {
      const next = clampBox({
        ...d.origin,
        w: d.origin.w + dx,
        h: d.origin.h + dy,
      })
      setBox(next)
    }
  }

  function onPointerUp() {
    if (!drag.current) return
    drag.current = null
    setBox((b) => {
      const next = clampBox(b)
      saveBox(next)
      return next
    })
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', onPointerUp)
  }

  function startDrag(
    kind: 'move' | 'resize',
    ev: ReactPointerEvent,
  ) {
    ev.preventDefault()
    ev.stopPropagation()
    drag.current = {
      kind,
      startX: ev.clientX,
      startY: ev.clientY,
      origin: box,
    }
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
  }

  async function send() {
    const text = input.trim()
    if (!text || busy || !ready) return
    const next: AgentChatMessage[] = [...messages, { role: 'user', content: text }]
    setMessages(next)
    setInput('')
    setBusy(true)
    setError(null)
    try {
      const res = await agentChat({
        messages: next,
        workflow,
        selection: { node_ids: pinnedIds },
        locale,
      })
      setMessages([...next, { role: 'assistant', content: res.message }])
      if (res.ops.length > 0) onOps(res.ops, res.message)
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      {open && (
        <aside
          className="agent-widget"
          style={{
            left: box.x,
            top: box.y,
            width: box.w,
            height: box.h,
          }}
          role="dialog"
          aria-label={t('agent.title')}
        >
          <header
            className="agent-widget__header"
            onPointerDown={(e) => {
              if ((e.target as HTMLElement).closest('button')) return
              startDrag('move', e)
            }}
          >
            <div className="agent-chat__title">
              <span className="agent-chat__avatar">
                <IconSparkle size={14} />
              </span>
              <div>
                <strong>{t('agent.title')}</strong>
                <span>{settings?.model || settings?.default_model || (ready ? t('settings.agent.connectionReady') : t('settings.agent.notConfigured'))}</span>
              </div>
            </div>
            <button
              type="button"
              className="icon-btn"
              onClick={onClose}
              aria-label={t('agent.close')}
            >
              <IconClose size={14} />
            </button>
          </header>

          <div className="agent-widget__body">
            {!ready ? (
              <div className="empty">
                <span className="empty__icon">
                  <IconSparkle size={22} />
                </span>
                <strong className="empty__title">{t('agent.disabledTitle')}</strong>
                <p className="empty__hint">{t('agent.disabledHint')}</p>
                <div className="empty__action">
                  <button type="button" className="btn btn--primary" onClick={onOpenSettings}>
                    {t('agent.openSettings')}
                  </button>
                </div>
              </div>
            ) : (
              <>
                <p className="agent-chat__hint">{t('agent.hint')}</p>
                <div className="agent-chat__pins">
                  <button
                    type="button"
                    className={`btn btn--sm${selectMode ? ' btn--ai' : ''}`}
                    onClick={onToggleSelectMode}
                  >
                    {selectMode ? t('agent.selecting') : t('agent.selectMode')}
                  </button>
                  {pinnedIds.length > 0 && (
                    <button
                      type="button"
                      className="btn btn--ghost btn--sm"
                      onClick={onClearPins}
                    >
                      {t('agent.clearPins')}
                    </button>
                  )}
                </div>
                {pinnedLabels.length > 0 && (
                  <div className="agent-chat__chips">
                    {pinnedLabels.map((n) => {
                      const key = labelKeyForType(n.typeId)
                      return (
                        <button
                          key={n.id}
                          type="button"
                          className="editor__chip editor__chip--active"
                          onClick={() => onRemovePin(n.id)}
                          title={t('agent.unpin')}
                        >
                          {key ? t(key) : n.typeId} · {n.id}
                        </button>
                      )
                    })}
                  </div>
                )}
                {selectMode && (
                  <p className="agent-chat__hint">{t('agent.selectHint')}</p>
                )}

                <div className="agent-chat__log" ref={scroller}>
                  {messages.length === 0 && (
                    <div className="agent-chat__welcome">
                      <p>{t('agent.empty')}</p>
                      <div className="agent-chat__suggestions">
                        {(['agent.suggest.1', 'agent.suggest.2', 'agent.suggest.3'] as const).map((key) => (
                          <button key={key} type="button" className="filter-chip" onClick={() => setInput(t(key))}>
                            {t(key)}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                  {messages.map((m, i) => (
                    <div
                      key={i}
                      className={`agent-chat__bubble agent-chat__bubble--${m.role === 'assistant' ? 'assistant' : 'user'}`}
                    >
                      {m.content}
                    </div>
                  ))}
                  {busy && (
                    <div className="agent-chat__bubble agent-chat__bubble--assistant is-pending">
                      <span className="typing"><i /><i /><i /></span>
                      {t('agent.thinking')}
                    </div>
                  )}
                </div>

                {error && <p className="editor__error">{error}</p>}

                <div className="agent-chat__composer">
                  <textarea
                    rows={2}
                    value={input}
                    disabled={busy}
                    placeholder={t('agent.placeholder')}
                    onChange={(e) => setInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && !e.shiftKey) {
                        e.preventDefault()
                        void send()
                      }
                    }}
                  />
                  <button
                    type="button"
                    className="btn btn--primary agent-chat__send"
                    disabled={busy || !input.trim()}
                    onClick={() => void send()}
                    aria-label={t('agent.send')}
                  >
                    <IconSend size={14} />
                  </button>
                </div>
              </>
            )}
          </div>

          <div
            className="agent-widget__resize"
            onPointerDown={(e) => startDrag('resize', e)}
            aria-hidden
          />
        </aside>
      )}
    </>
  )
}
