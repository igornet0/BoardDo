import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { TYPE_IDS } from '../types'
import type { Connection } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import { ExpressionField } from '../ui/ExpressionField'

type Config = Record<string, unknown>

interface FormProps {
  config: Config
  onChange: (next: Config) => void
  connections: Connection[]
}

function asString(v: unknown, fallback = ''): string {
  if (v == null) return fallback
  if (typeof v === 'string') return v
  if (typeof v === 'number' || typeof v === 'boolean') return String(v)
  return JSON.stringify(v)
}

function asNumber(v: unknown, fallback: number): number {
  if (typeof v === 'number' && Number.isFinite(v)) return v
  if (typeof v === 'string' && v.trim() !== '' && !Number.isNaN(Number(v))) {
    return Number(v)
  }
  return fallback
}

function asObject(v: unknown): Record<string, unknown> {
  if (v && typeof v === 'object' && !Array.isArray(v)) {
    return v as Record<string, unknown>
  }
  return {}
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <label className="editor__field">
      <span>{label}</span>
      {hint ? <span className="editor__hint">{hint}</span> : null}
      {children}
    </label>
  )
}

function InfoBlock({ children }: { children: ReactNode }) {
  return <div className="editor__info">{children}</div>
}

function ConnectionField({
  config,
  onChange,
  connections,
  type,
}: {
  config: Config
  onChange: (next: Config) => void
  connections: Connection[]
  type: 'http' | 'telegram' | 'openai' | 'github'
}) {
  const { t } = usePreferences()
  const filtered = connections.filter(
    (c) =>
      c.enabled &&
      (c.type === type || (type === 'openai' && c.type === 'ai')),
  )

  return (
    <Field
      label={t('editor.connection')}
      hint={
        filtered.length === 0
          ? t('editor.noConnection')
          : t('editor.connectionHint')
      }
    >
      <select
        value={asString(config.connection_id)}
        onChange={(e) => {
          const next = { ...config }
          if (e.target.value) next.connection_id = e.target.value
          else delete next.connection_id
          onChange(next)
        }}
      >
        <option value="">{t('editor.none')}</option>
        {filtered.map((c) => {
          const model = String(
            (c.config as Record<string, unknown> | undefined)?.default_model ??
              '',
          )
          return (
            <option key={c.id} value={c.id}>
              {c.name}
              {model ? ` · ${model}` : ''}
              {c.has_secret ? ` · ${t('connections.secret')}` : ''}
            </option>
          )
        })}
      </select>
    </Field>
  )
}

type KvRow = { id: string; key: string; value: string }

function rowsFromObject(value: Record<string, unknown>): KvRow[] {
  return Object.entries(value).map(([k, v]) => ({
    id: crypto.randomUUID(),
    key: k,
    value: asString(v),
  }))
}

function objectFromRows(rows: KvRow[]): Record<string, unknown> {
  const next: Record<string, unknown> = {}
  for (const row of rows) {
    const k = row.key.trim()
    if (!k) continue
    next[k] = row.value
  }
  return next
}

/** Editable string→string map (headers, query, transform mapping). */
function KeyValueEditor({
  label,
  hint,
  value,
  onChange,
  keyPlaceholder,
  valuePlaceholder,
  addLabel,
}: {
  label: string
  hint?: string
  value: Record<string, unknown>
  onChange: (next: Record<string, unknown>) => void
  keyPlaceholder: string
  valuePlaceholder: string
  addLabel: string
}) {
  const [rows, setRows] = useState<KvRow[]>(() => rowsFromObject(value))
  const [syncKey, setSyncKey] = useState(() => JSON.stringify(value))

  useEffect(() => {
    const nextKey = JSON.stringify(value)
    if (nextKey === syncKey) return
    // External config change (node switch / JSON paste) — resync rows.
    setRows(rowsFromObject(value))
    setSyncKey(nextKey)
  }, [value, syncKey])

  const commit = (nextRows: KvRow[]) => {
    setRows(nextRows)
    const obj = objectFromRows(nextRows)
    setSyncKey(JSON.stringify(obj))
    onChange(obj)
  }

  return (
    <div className="editor__field">
      <span>{label}</span>
      {hint ? <span className="editor__hint">{hint}</span> : null}
      <div className="editor__kv">
        {rows.length === 0 && <p className="editor__kv-empty">—</p>}
        {rows.map((row, index) => (
          <div key={row.id} className="editor__kv-row">
            <input
              value={row.key}
              placeholder={keyPlaceholder}
              onChange={(e) => {
                const next = rows.map((r) => ({ ...r }))
                next[index] = { ...next[index], key: e.target.value }
                commit(next)
              }}
            />
            <input
              value={row.value}
              placeholder={valuePlaceholder}
              onChange={(e) => {
                const next = rows.map((r) => ({ ...r }))
                next[index] = { ...next[index], value: e.target.value }
                commit(next)
              }}
            />
            <button
              type="button"
              className="btn btn--ghost editor__kv-remove"
              aria-label="Remove"
              onClick={() => commit(rows.filter((_, i) => i !== index))}
            >
              ✕
            </button>
          </div>
        ))}
        <button
          type="button"
          className="btn btn--ghost editor__kv-add"
          onClick={() =>
            commit([
              ...rows,
              { id: crypto.randomUUID(), key: '', value: '' },
            ])
          }
        >
          {addLabel}
        </button>
      </div>
    </div>
  )
}

function ManualForm() {
  const { t } = usePreferences()
  return <InfoBlock>{t('editor.manual.help')}</InfoBlock>
}

function WebhookForm() {
  const { t } = usePreferences()
  return (
    <InfoBlock>
      <p>{t('editor.webhook.help')}</p>
      <code>POST /api/webhooks/:workflow_id</code>
      <p className="editor__hint">{t('editor.webhook.body')}</p>
    </InfoBlock>
  )
}

function ScheduleForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()

  const mode: 'every' | 'daily' | 'cron' | 'after' = config.after_completion
    ? 'after'
    : config.cron
      ? 'cron'
      : config.daily_at
        ? 'daily'
        : 'every'

  const every = asObject(config.every)
  const everyUnit =
    every.seconds != null ? 'seconds' : every.hours != null ? 'hours' : 'minutes'
  const everyValue = asNumber(
    everyUnit === 'seconds'
      ? every.seconds
      : everyUnit === 'hours'
        ? every.hours
        : every.minutes,
    everyUnit === 'hours' ? 1 : everyUnit === 'seconds' ? 10 : 5,
  )
  const overlap = asString(config.on_overlap, 'skip') === 'queue' ? 'queue' : 'skip'

  const setMode = (next: 'every' | 'daily' | 'cron' | 'after') => {
    const timezone = asString(config.timezone, 'UTC')
    if (next === 'every') {
      onChange({ timezone, every: { minutes: 5 }, on_overlap: overlap })
    } else if (next === 'daily') {
      onChange({ timezone, daily_at: '09:00', on_overlap: overlap })
    } else if (next === 'after') {
      onChange({
        timezone,
        after_completion: { seconds: 30 },
        on_overlap: overlap,
      })
    } else {
      onChange({ timezone, cron: '*/5 * * * *', on_overlap: overlap })
    }
  }

  const afterSecs = asNumber(
    asObject(config.after_completion).seconds ?? config.after_completion,
    30,
  )

  return (
    <>
      <InfoBlock>{t('editor.schedule.help')}</InfoBlock>
      <Field label={t('editor.schedule.mode')}>
        <div className="editor__segment">
          {(['every', 'daily', 'cron', 'after'] as const).map((m) => (
            <button
              key={m}
              type="button"
              className={
                mode === m
                  ? 'editor__segment-btn editor__segment-btn--active'
                  : 'editor__segment-btn'
              }
              onClick={() => setMode(m)}
            >
              {t(`editor.schedule.mode.${m}` as MessageKey)}
            </button>
          ))}
        </div>
      </Field>

      {mode === 'every' && (
        <div className="editor__row">
          <Field label={t('editor.schedule.every')}>
            <input
              type="number"
              min={1}
              max={
                everyUnit === 'hours' ? 23 : everyUnit === 'seconds' ? 3600 : 59
              }
              value={everyValue}
              onChange={(e) => {
                const n = Math.max(1, Number(e.target.value) || 1)
                onChange({
                  ...config,
                  every:
                    everyUnit === 'hours'
                      ? { hours: n }
                      : everyUnit === 'seconds'
                        ? { seconds: n }
                        : { minutes: n },
                })
              }}
            />
          </Field>
          <Field label={t('editor.schedule.unit')}>
            <select
              value={everyUnit}
              onChange={(e) => {
                const unit = e.target.value as 'seconds' | 'minutes' | 'hours'
                const n = everyValue
                onChange({
                  ...config,
                  every:
                    unit === 'hours'
                      ? { hours: n }
                      : unit === 'seconds'
                        ? { seconds: n }
                        : { minutes: n },
                })
              }}
            >
              <option value="seconds">{t('editor.schedule.seconds')}</option>
              <option value="minutes">{t('editor.schedule.minutes')}</option>
              <option value="hours">{t('editor.schedule.hours')}</option>
            </select>
          </Field>
        </div>
      )}

      {mode === 'daily' && (
        <Field label={t('editor.schedule.dailyAt')} hint="HH:MM">
          <input
            type="time"
            value={asString(config.daily_at, '09:00')}
            onChange={(e) =>
              onChange({ ...config, daily_at: e.target.value })
            }
          />
        </Field>
      )}

      {mode === 'cron' && (
        <Field
          label={t('editor.schedule.cron')}
          hint={t('editor.schedule.cronHint')}
        >
          <input
            value={asString(config.cron)}
            onChange={(e) => onChange({ ...config, cron: e.target.value })}
            spellCheck={false}
            placeholder="*/5 * * * *"
          />
        </Field>
      )}

      {mode === 'after' && (
        <Field label={t('editor.schedule.afterSeconds')}>
          <input
            type="number"
            min={1}
            value={afterSecs}
            onChange={(e) =>
              onChange({
                ...config,
                after_completion: {
                  seconds: Math.max(1, Number(e.target.value) || 1),
                },
              })
            }
          />
        </Field>
      )}

      <Field label={t('editor.schedule.timezone')}>
        <input
          value={asString(config.timezone, 'UTC')}
          onChange={(e) => onChange({ ...config, timezone: e.target.value })}
          placeholder="UTC"
          spellCheck={false}
        />
      </Field>
      <Field label={t('editor.schedule.overlap')}>
        <select
          value={overlap}
          onChange={(e) =>
            onChange({ ...config, on_overlap: e.target.value })
          }
        >
          <option value="skip">{t('editor.schedule.overlap.skip')}</option>
          <option value="queue">{t('editor.schedule.overlap.queue')}</option>
        </select>
      </Field>
    </>
  )
}

function SetVariableForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <Field label={t('editor.set.name')} hint={t('editor.set.nameHint')}>
        <input
          value={asString(config.name)}
          onChange={(e) => onChange({ ...config, name: e.target.value })}
          placeholder="amount"
          spellCheck={false}
        />
      </Field>
      <Field label={t('editor.set.value')} hint={t('editor.exprHint')}>
        <ExpressionField
          multiline
          rows={4}
          value={asString(config.value)}
          placeholder="{{trigger.amount}}"
          onChange={(raw) => {
            let value: unknown = raw
            if (/^-?\d+(\.\d+)?$/.test(raw.trim())) value = Number(raw.trim())
            else if (raw.trim() === 'true') value = true
            else if (raw.trim() === 'false') value = false
            else if (raw.trim() === 'null') value = null
            onChange({ ...config, value })
          }}
        />
      </Field>
    </>
  )
}

function TransformForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  const mapping = asObject(config.mapping)
  return (
    <KeyValueEditor
      label={t('editor.transform.mapping')}
      hint={t('editor.transform.hint')}
      value={mapping}
      onChange={(next) => onChange({ ...config, mapping: next })}
      keyPlaceholder={t('editor.transform.field')}
      valuePlaceholder="{{trigger.x}}"
      addLabel={t('editor.transform.add')}
    />
  )
}

function ConditionForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  const mode: 'expression' | 'compare' =
    config.expression != null || config.left == null ? 'expression' : 'compare'

  const setMode = (next: 'expression' | 'compare') => {
    if (next === 'expression') {
      onChange({ expression: asString(config.expression, '{{amount > 100}}') })
    } else {
      onChange({
        left: asString(config.left, '{{amount}}'),
        operator: asString(config.operator, '>'),
        right: config.right ?? 100,
      })
    }
  }

  return (
    <>
      <Field label={t('editor.condition.mode')}>
        <div className="editor__segment">
          <button
            type="button"
            className={
              mode === 'expression'
                ? 'editor__segment-btn editor__segment-btn--active'
                : 'editor__segment-btn'
            }
            onClick={() => setMode('expression')}
          >
            {t('editor.condition.expression')}
          </button>
          <button
            type="button"
            className={
              mode === 'compare'
                ? 'editor__segment-btn editor__segment-btn--active'
                : 'editor__segment-btn'
            }
            onClick={() => setMode('compare')}
          >
            {t('editor.condition.compare')}
          </button>
        </div>
      </Field>

      {mode === 'expression' ? (
        <Field
          label={t('editor.condition.expression')}
          hint={t('editor.exprHint')}
        >
          <ExpressionField
            value={asString(config.expression)}
            onChange={(expression) => onChange({ expression })}
            placeholder="{{amount > 100}}"
          />
        </Field>
      ) : (
        <>
          <Field label={t('editor.condition.left')} hint={t('editor.exprHint')}>
            <input
              value={asString(config.left)}
              onChange={(e) => onChange({ ...config, left: e.target.value })}
              placeholder="{{amount}}"
              spellCheck={false}
            />
          </Field>
          <Field label={t('editor.condition.operator')}>
            <select
              value={asString(config.operator, '>')}
              onChange={(e) => onChange({ ...config, operator: e.target.value })}
            >
              {['==', '!=', '>', '>=', '<', '<=', 'contains'].map((op) => (
                <option key={op} value={op}>
                  {op}
                </option>
              ))}
            </select>
          </Field>
          <Field label={t('editor.condition.right')}>
            <input
              value={asString(config.right)}
              onChange={(e) => {
                const raw = e.target.value
                let right: unknown = raw
                if (/^-?\d+(\.\d+)?$/.test(raw.trim())) right = Number(raw.trim())
                onChange({ ...config, right })
              }}
              placeholder="100"
              spellCheck={false}
            />
          </Field>
        </>
      )}
      <InfoBlock>{t('editor.condition.ports')}</InfoBlock>
    </>
  )
}

function DelayForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  const ms = asNumber(config.ms, 500)
  const presets = [100, 500, 1000, 3000, 5000]

  return (
    <>
      <Field label={t('editor.delay.ms')} hint={t('editor.delay.hint')}>
        <input
          type="number"
          min={0}
          step={50}
          value={ms}
          onChange={(e) =>
            onChange({ ...config, ms: Math.max(0, Number(e.target.value) || 0) })
          }
        />
      </Field>
      <div className="editor__chips">
        {presets.map((p) => (
          <button
            key={p}
            type="button"
            className={
              ms === p
                ? 'editor__chip editor__chip--active'
                : 'editor__chip'
            }
            onClick={() => onChange({ ...config, ms: p })}
          >
            {p < 1000 ? `${p} ms` : `${p / 1000} s`}
          </button>
        ))}
      </div>
    </>
  )
}

function HttpForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  const retry = asObject(config.retry)
  const serializedBody =
    typeof config.body === 'string'
      ? config.body
      : config.body != null
        ? JSON.stringify(config.body, null, 2)
        : ''
  const [bodyText, setBodyText] = useState(serializedBody)
  const [bodySync, setBodySync] = useState(serializedBody)

  useEffect(() => {
    if (serializedBody === bodySync) return
    setBodyText(serializedBody)
    setBodySync(serializedBody)
  }, [serializedBody, bodySync])

  return (
    <>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="http"
      />
      <div className="editor__row">
        <Field label={t('editor.http.method')}>
          <select
            value={asString(config.method, 'GET').toUpperCase()}
            onChange={(e) => onChange({ ...config, method: e.target.value })}
          >
            {['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t('editor.http.timeout')}>
          <input
            type="number"
            min={100}
            step={100}
            value={asNumber(config.timeout_ms, 10000)}
            onChange={(e) =>
              onChange({
                ...config,
                timeout_ms: Math.max(100, Number(e.target.value) || 10000),
              })
            }
          />
        </Field>
      </div>
      <Field label={t('editor.http.url')} hint={t('editor.http.urlHint')}>
        <input
          value={asString(config.url)}
          onChange={(e) => onChange({ ...config, url: e.target.value })}
          placeholder="https://api.example.com/items"
          spellCheck={false}
        />
      </Field>
      <KeyValueEditor
        label={t('editor.http.headers')}
        value={asObject(config.headers)}
        onChange={(headers) => onChange({ ...config, headers })}
        keyPlaceholder="Authorization"
        valuePlaceholder="Bearer …"
        addLabel={t('editor.http.addHeader')}
      />
      <KeyValueEditor
        label={t('editor.http.query')}
        value={asObject(config.query)}
        onChange={(query) => onChange({ ...config, query })}
        keyPlaceholder="limit"
        valuePlaceholder="10"
        addLabel={t('editor.http.addQuery')}
      />
      <Field label={t('editor.http.body')} hint={t('editor.http.bodyHint')}>
        <textarea
          rows={6}
          value={bodyText}
          onChange={(e) => {
            const raw = e.target.value
            setBodyText(raw)
            if (!raw.trim()) {
              const next = { ...config }
              delete next.body
              setBodySync('')
              onChange(next)
              return
            }
            try {
              const parsed = JSON.parse(raw) as unknown
              setBodySync(JSON.stringify(parsed, null, 2))
              onChange({ ...config, body: parsed })
            } catch {
              setBodySync(raw)
              onChange({ ...config, body: raw })
            }
          }}
          spellCheck={false}
          placeholder='{ "name": "{{trigger.name}}" }'
        />
      </Field>
      <div className="editor__row">
        <Field label={t('editor.http.retryMax')}>
          <input
            type="number"
            min={0}
            max={10}
            value={asNumber(retry.max, 0)}
            onChange={(e) =>
              onChange({
                ...config,
                retry: {
                  ...retry,
                  max: Math.max(0, Number(e.target.value) || 0),
                },
              })
            }
          />
        </Field>
        <Field label={t('editor.http.retryBackoff')}>
          <input
            type="number"
            min={0}
            step={50}
            value={asNumber(retry.backoff_ms, 300)}
            onChange={(e) =>
              onChange({
                ...config,
                retry: {
                  ...retry,
                  backoff_ms: Math.max(0, Number(e.target.value) || 0),
                },
              })
            }
          />
        </Field>
      </div>
    </>
  )
}

function TelegramMessageForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="telegram"
      />
      <Field label={t('editor.telegram.chatId')} hint={t('editor.exprHint')}>
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
          placeholder="{{variables.chat_id}}"
          spellCheck={false}
        />
      </Field>
      <Field label={t('editor.telegram.text')} hint={t('editor.exprHint')}>
        <ExpressionField
          multiline
          rows={5}
          value={asString(config.text)}
          onChange={(text) => onChange({ ...config, text })}
          placeholder="{{trigger.message}}"
        />
      </Field>
      <TelegramParseModeField config={config} onChange={onChange} />
    </>
  )
}

function TelegramMediaForm({
  config,
  onChange,
  connections,
  mediaKey,
}: FormProps & { mediaKey: 'photo' | 'document' }) {
  const { t } = usePreferences()
  return (
    <>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="telegram"
      />
      <Field label={t('editor.telegram.chatId')} hint={t('editor.exprHint')}>
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
          placeholder="{{variables.chat_id}}"
          spellCheck={false}
        />
      </Field>
      <Field
        label={
          mediaKey === 'photo'
            ? t('editor.telegram.photo')
            : t('editor.telegram.document')
        }
        hint={t('editor.telegram.mediaHint')}
      >
        <input
          value={asString(config[mediaKey])}
          onChange={(e) => onChange({ ...config, [mediaKey]: e.target.value })}
          placeholder={
            mediaKey === 'photo'
              ? 'https://…/image.jpg'
              : 'https://…/file.pdf'
          }
          spellCheck={false}
        />
      </Field>
      <Field label={t('editor.telegram.caption')} hint={t('editor.exprHint')}>
        <textarea
          rows={3}
          value={asString(config.caption)}
          onChange={(e) => onChange({ ...config, caption: e.target.value })}
          placeholder="{{trigger.caption}}"
        />
      </Field>
    </>
  )
}

function LogForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <Field label={t('editor.log.message')} hint={t('editor.exprHint')}>
      <ExpressionField
        multiline
        rows={4}
        value={asString(config.message)}
        onChange={(message) => onChange({ ...config, message })}
        placeholder="{{message}}"
      />
    </Field>
  )
}

function TelegramUserAccountField({
  config,
  onChange,
  allowAny,
}: {
  config: Config
  onChange: (next: Config) => void
  allowAny?: boolean
}) {
  const { t } = usePreferences()
  const [accounts, setAccounts] = useState<{ id: string; label: string }[]>([])

  useEffect(() => {
    void import('../api/telegram')
      .then((mod) => mod.listTelegramAccounts())
      .then((list) =>
        setAccounts(
          list.map((a) => ({
            id: a.id,
            label:
              a.display_name || a.username || a.phone_masked || a.id.slice(0, 8),
          })),
        ),
      )
      .catch(() => setAccounts([]))
  }, [])

  return (
    <Field
      label={t('editor.telegramUser.account')}
      hint={
        accounts.length === 0
          ? t('editor.telegramUser.noAccounts')
          : t('editor.telegramUser.accountHint')
      }
    >
      <select
        value={asString(config.account_id, allowAny ? 'any' : '')}
        onChange={(e) => onChange({ ...config, account_id: e.target.value })}
      >
        {allowAny && (
          <option value="any">{t('editor.telegramUser.any')}</option>
        )}
        {!allowAny && <option value="">{t('editor.none')}</option>}
        {accounts.map((a) => (
          <option key={a.id} value={a.id}>
            {a.label}
          </option>
        ))}
      </select>
    </Field>
  )
}

function chatIdListToArray(v: unknown): string[] {
  if (Array.isArray(v)) {
    return v.map((item) => String(item).trim()).filter(Boolean)
  }
  if (typeof v === 'string' && v.trim()) {
    return parseChatIdListInput(v)
  }
  return []
}

/** Unique chat IDs as strings (keeps full i64 precision; backend parses both). */
function parseChatIdListInput(raw: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const part of raw.split(/[\s,;]+/)) {
    const id = part.trim()
    if (!id || seen.has(id)) continue
    if (!/^-?\d+$/.test(id)) continue
    seen.add(id)
    out.push(id)
  }
  return out
}

type ChatIdRow = { id: string; value: string }

function rowsFromChatIds(ids: string[]): ChatIdRow[] {
  return ids.map((value) => ({ id: crypto.randomUUID(), value }))
}

function ChatIdTableEditor({
  label,
  hint,
  value,
  onChange,
  otherIds = [],
  addLabel,
  placeholder,
  duplicateHint,
  conflictHint,
}: {
  label: string
  hint?: string
  value: unknown
  onChange: (next: string[]) => void
  /** IDs from the sibling list (allow ↔ ignore) for cross-highlight. */
  otherIds?: string[]
  addLabel: string
  placeholder: string
  duplicateHint: string
  conflictHint: string
}) {
  const ids = chatIdListToArray(value)
  const [rows, setRows] = useState<ChatIdRow[]>(() => rowsFromChatIds(ids))
  const [syncKey, setSyncKey] = useState(() => JSON.stringify(ids))

  useEffect(() => {
    const nextKey = JSON.stringify(ids)
    if (nextKey === syncKey) return
    setRows(rowsFromChatIds(ids))
    setSyncKey(nextKey)
  }, [ids, syncKey])

  const otherSet = useMemo(
    () => new Set(otherIds.map((id) => id.trim()).filter(Boolean)),
    [otherIds],
  )

  const counts = useMemo(() => {
    const map = new Map<string, number>()
    for (const row of rows) {
      const v = row.value.trim()
      if (!v) continue
      map.set(v, (map.get(v) ?? 0) + 1)
    }
    return map
  }, [rows])

  const commit = (nextRows: ChatIdRow[]) => {
    setRows(nextRows)
    const seen = new Set<string>()
    const out: string[] = []
    for (const row of nextRows) {
      const id = row.value.trim()
      if (!id || seen.has(id)) continue
      if (!/^-?\d+$/.test(id)) continue
      seen.add(id)
      out.push(id)
    }
    setSyncKey(JSON.stringify(out))
    onChange(out)
  }

  return (
    <div className="editor__field">
      <span>{label}</span>
      {hint ? <span className="editor__hint">{hint}</span> : null}
      <div className="editor__id-table">
        <div className="editor__id-table__head" aria-hidden>
          <span>Chat ID</span>
          <span />
        </div>
        {rows.length === 0 && (
          <p className="editor__kv-empty">{'—'}</p>
        )}
        {rows.map((row, index) => {
          const trimmed = row.value.trim()
          const isDup = trimmed !== '' && (counts.get(trimmed) ?? 0) > 1
          const isConflict = trimmed !== '' && otherSet.has(trimmed)
          const isInvalid =
            trimmed !== '' && !/^-?\d+$/.test(trimmed)
          const rowClass = [
            'editor__id-row',
            isDup || isConflict ? 'editor__id-row--warn' : '',
            isInvalid ? 'editor__id-row--invalid' : '',
          ]
            .filter(Boolean)
            .join(' ')

          return (
            <div key={row.id} className={rowClass}>
              <input
                className="editor__id-row__input"
                value={row.value}
                placeholder={placeholder}
                spellCheck={false}
                inputMode="numeric"
                onChange={(e) => {
                  const next = rows.map((r) => ({ ...r }))
                  next[index] = { ...next[index], value: e.target.value }
                  commit(next)
                }}
              />
              <button
                type="button"
                className="btn btn--ghost editor__kv-remove"
                aria-label="Remove"
                onClick={() => commit(rows.filter((_, i) => i !== index))}
              >
                ✕
              </button>
              {(isDup || isConflict || isInvalid) && (
                <span className="editor__id-row__badge">
                  {isInvalid
                    ? '!'
                    : isDup
                      ? duplicateHint
                      : conflictHint}
                </span>
              )}
            </div>
          )
        })}
        <button
          type="button"
          className="btn btn--ghost editor__kv-add"
          onClick={() =>
            commit([...rows, { id: crypto.randomUUID(), value: '' }])
          }
        >
          {addLabel}
        </button>
      </div>
    </div>
  )
}

function TelegramUserTriggerForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  const ignoreOutgoing = config.ignore_outgoing !== false
  const onlyIds = chatIdListToArray(
    config.only_chat_ids ?? config.chat_ids,
  )
  const ignoreIds = chatIdListToArray(
    config.ignore_chat_ids ?? config.ignore_chats,
  )

  return (
    <>
      <InfoBlock>{t('editor.telegramUser.triggerHelp')}</InfoBlock>
      <TelegramUserAccountField config={config} onChange={onChange} allowAny />
      <Field
        label={t('editor.telegram.chatId')}
        hint={t('editor.telegramUser.chatIdHint')}
      >
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
          placeholder="optional"
        />
      </Field>
      <ChatIdTableEditor
        label={t('editor.telegramUser.onlyChatIds')}
        hint={t('editor.telegramUser.onlyChatIdsHint')}
        value={config.only_chat_ids ?? config.chat_ids}
        otherIds={ignoreIds}
        addLabel={t('editor.telegramUser.addChatId')}
        placeholder="-1001234567890"
        duplicateHint={t('editor.telegramUser.duplicateId')}
        conflictHint={t('editor.telegramUser.conflictId')}
        onChange={(only_chat_ids) =>
          onChange({
            ...config,
            only_chat_ids,
            chat_ids: undefined,
          })
        }
      />
      <ChatIdTableEditor
        label={t('editor.telegramUser.ignoreChatIds')}
        hint={t('editor.telegramUser.ignoreChatIdsHint')}
        value={config.ignore_chat_ids ?? config.ignore_chats}
        otherIds={onlyIds}
        addLabel={t('editor.telegramUser.addChatId')}
        placeholder="-1009876543210"
        duplicateHint={t('editor.telegramUser.duplicateId')}
        conflictHint={t('editor.telegramUser.conflictId')}
        onChange={(ignore_chat_ids) =>
          onChange({
            ...config,
            ignore_chat_ids,
            ignore_chats: undefined,
          })
        }
      />
      <Field label={t('editor.telegramUser.textContains')}>
        <input
          value={asString(config.text_contains)}
          onChange={(e) =>
            onChange({ ...config, text_contains: e.target.value })
          }
        />
      </Field>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={ignoreOutgoing}
          onChange={(e) =>
            onChange({ ...config, ignore_outgoing: e.target.checked })
          }
        />
        {t('editor.telegramUser.ignoreOutgoing')}
      </label>
      <p className="editor__hint">{t('editor.telegramUser.ignoreOutgoingHint')}</p>
    </>
  )
}

function TelegramParseModeField({
  config,
  onChange,
}: {
  config: Record<string, unknown>
  onChange: (next: Record<string, unknown>) => void
}) {
  const { t } = usePreferences()
  return (
    <Field
      label={t('editor.telegram.parseMode')}
      hint={t('editor.telegram.parseModeHint')}
    >
      <select
        value={asString(config.parse_mode) || 'markdown'}
        onChange={(e) => onChange({ ...config, parse_mode: e.target.value })}
      >
        <option value="markdown">{t('editor.telegram.parseModeMarkdown')}</option>
        <option value="html">{t('editor.telegram.parseModeHtml')}</option>
        <option value="plain">{t('editor.telegram.parseModePlain')}</option>
      </select>
    </Field>
  )
}

function TelegramUserSendForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <TelegramUserAccountField config={config} onChange={onChange} />
      <Field label={t('editor.telegram.chatId')} hint={t('editor.exprHint')}>
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
        />
      </Field>
      <Field label={t('editor.telegram.text')} hint={t('editor.exprHint')}>
        <ExpressionField
          multiline
          rows={5}
          value={asString(config.text)}
          onChange={(text) => onChange({ ...config, text })}
        />
      </Field>
      <TelegramParseModeField config={config} onChange={onChange} />
      <Field
        label={t('editor.telegramUser.replyTo')}
        hint={t('editor.telegramUser.replyToHint')}
      >
        <input
          value={asString(config.reply_to_message_id)}
          placeholder="{{trigger.message_id}}"
          onChange={(e) =>
            onChange({ ...config, reply_to_message_id: e.target.value })
          }
        />
      </Field>
    </>
  )
}

function TelegramUserMembersForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.telegramUser.membersHelp')}</InfoBlock>
      <TelegramUserAccountField config={config} onChange={onChange} />
      <Field label={t('editor.telegram.chatId')} hint={t('editor.exprHint')}>
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
        />
      </Field>
      <Field label={t('editor.telegramUser.membersLimit')}>
        <input
          type="number"
          min={1}
          max={1000}
          value={asNumber(config.limit, 200)}
          onChange={(e) => {
            const n = Math.min(1000, Math.max(1, Number(e.target.value) || 200))
            onChange({ ...config, limit: n })
          }}
        />
      </Field>
      <Field
        label={t('editor.telegramUser.membersCacheHours')}
        hint={t('editor.telegramUser.membersCacheHoursHint')}
      >
        <input
          type="number"
          min={0}
          step={1}
          value={asNumber(config.cache_ttl_hours, 24)}
          onChange={(e) => {
            const n = Math.max(0, Number(e.target.value) || 0)
            onChange({ ...config, cache_ttl_hours: n })
          }}
        />
      </Field>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.continue_on_error === true}
          onChange={(e) =>
            onChange({ ...config, continue_on_error: e.target.checked })
          }
        />
        {t('editor.telegramUser.continueOnError')}
      </label>
      <p className="editor__hint">{t('editor.telegramUser.continueOnErrorHint')}</p>
    </>
  )
}

function TelegramUserForwardForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <TelegramUserAccountField config={config} onChange={onChange} />
      <Field label={t('editor.telegramUser.fromChat')}>
        <input
          value={asString(config.from_chat_id)}
          onChange={(e) =>
            onChange({ ...config, from_chat_id: e.target.value })
          }
        />
      </Field>
      <Field label={t('editor.telegramUser.toChat')}>
        <input
          value={asString(config.to_chat_id)}
          onChange={(e) => onChange({ ...config, to_chat_id: e.target.value })}
        />
      </Field>
      <Field label={t('editor.telegramUser.messageId')}>
        <input
          value={asString(config.message_id)}
          onChange={(e) => onChange({ ...config, message_id: e.target.value })}
        />
      </Field>
    </>
  )
}

function TelegramUserEditForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <TelegramUserAccountField config={config} onChange={onChange} />
      <Field label={t('editor.telegram.chatId')}>
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
        />
      </Field>
      <Field label={t('editor.telegramUser.messageId')}>
        <input
          value={asString(config.message_id)}
          onChange={(e) => onChange({ ...config, message_id: e.target.value })}
        />
      </Field>
      <Field label={t('editor.telegram.text')}>
        <textarea
          rows={4}
          value={asString(config.text)}
          onChange={(e) => onChange({ ...config, text: e.target.value })}
        />
      </Field>
      <TelegramParseModeField config={config} onChange={onChange} />
    </>
  )
}

function TelegramUserDeleteForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <TelegramUserAccountField config={config} onChange={onChange} />
      <Field label={t('editor.telegram.chatId')}>
        <input
          value={asString(config.chat_id)}
          onChange={(e) => onChange({ ...config, chat_id: e.target.value })}
        />
      </Field>
      <Field label={t('editor.telegramUser.messageId')}>
        <input
          value={asString(config.message_id)}
          onChange={(e) => onChange({ ...config, message_id: e.target.value })}
        />
      </Field>
    </>
  )
}

function FallbackForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  const [text, setText] = useState(() => JSON.stringify(config, null, 2))
  const [error, setError] = useState<string | null>(null)

  return (
    <Field label={t('editor.config')} hint={t('editor.advancedHint')}>
      <textarea
        rows={10}
        value={text}
        spellCheck={false}
        onChange={(e) => {
          setText(e.target.value)
          try {
            const parsed = JSON.parse(e.target.value) as Config
            onChange(parsed)
            setError(null)
          } catch {
            setError(t('editor.invalidJson'))
          }
        }}
      />
      {error && <p className="editor__error">{error}</p>}
    </Field>
  )
}

export function NodeConfigForm({
  typeId,
  config,
  onChange,
  connections,
}: {
  typeId: string
  config: Config
  onChange: (next: Config) => void
  connections: Connection[]
}) {
  const props: FormProps = { config, onChange, connections }

  switch (typeId) {
    case TYPE_IDS.TRIGGER_MANUAL:
      return <ManualForm />
    case TYPE_IDS.TRIGGER_WEBHOOK:
      return <WebhookForm />
    case TYPE_IDS.TRIGGER_SCHEDULE:
      return <ScheduleForm {...props} />
    case TYPE_IDS.DATA_SET:
      return <SetVariableForm {...props} />
    case TYPE_IDS.DATA_TRANSFORM:
      return <TransformForm {...props} />
    case TYPE_IDS.LOGIC_CONDITION:
      return <ConditionForm {...props} />
    case TYPE_IDS.LOGIC_DELAY:
      return <DelayForm {...props} />
    case TYPE_IDS.HTTP_REQUEST:
      return <HttpForm {...props} />
    case TYPE_IDS.AI_CHAT:
      return <AiChatForm {...props} />
    case TYPE_IDS.AI_CLASSIFY:
      return <AiClassifyForm {...props} />
    case TYPE_IDS.AI_ANALYZE:
      return <AiChatForm {...props} />
    case TYPE_IDS.AI_IMAGE:
      return <AiImageForm {...props} />
    case TYPE_IDS.AI_AUDIO:
      return <AiAudioForm {...props} />
    case TYPE_IDS.AI_VIDEO:
      return <AiVideoForm {...props} />
    case TYPE_IDS.WEB_SEARCH:
      return <WebSearchForm {...props} />
    case TYPE_IDS.WEB_OPEN:
      return <WebOpenForm {...props} />
    case TYPE_IDS.WEB_EXTRACT:
      return <WebExtractForm {...props} />
    case TYPE_IDS.WEB_FETCH:
      return <HttpForm {...props} />
    case TYPE_IDS.GITHUB_GET_LATEST_RELEASE:
      return <GitHubLatestReleaseForm {...props} />
    case TYPE_IDS.TELEGRAM_SEND_MESSAGE:
      return <TelegramMessageForm {...props} />
    case TYPE_IDS.TELEGRAM_SEND_PHOTO:
      return <TelegramMediaForm {...props} mediaKey="photo" />
    case TYPE_IDS.TELEGRAM_SEND_DOCUMENT:
      return <TelegramMediaForm {...props} mediaKey="document" />
    case TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED:
      return <TelegramUserTriggerForm {...props} />
    case TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE:
      return <TelegramUserSendForm {...props} />
    case TYPE_IDS.TELEGRAM_USER_FORWARD_MESSAGE:
      return <TelegramUserForwardForm {...props} />
    case TYPE_IDS.TELEGRAM_USER_EDIT_MESSAGE:
      return <TelegramUserEditForm {...props} />
    case TYPE_IDS.TELEGRAM_USER_DELETE_MESSAGES:
      return <TelegramUserDeleteForm {...props} />
    case TYPE_IDS.TELEGRAM_USER_GET_CHAT_MEMBERS:
      return <TelegramUserMembersForm {...props} />
    case TYPE_IDS.DEBUG_LOG:
      return <LogForm {...props} />
    default:
      return <FallbackForm {...props} />
  }
}

function AiChatForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.ai.help')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="openai"
      />
      <ModelSelectField
        config={config}
        onChange={onChange}
        connections={connections}
        capability="chat"
      />
      <Field label={t('editor.ai.system')} hint={t('editor.exprHint')}>
        <textarea
          rows={3}
          value={asString(config.system)}
          onChange={(e) => onChange({ ...config, system: e.target.value })}
        />
      </Field>
      <Field label={t('editor.ai.prompt')} hint={t('editor.exprHint')}>
        <ExpressionField
          multiline
          rows={4}
          value={asString(config.prompt)}
          onChange={(prompt) => onChange({ ...config, prompt })}
        />
      </Field>
      <Field
        label={t('editor.ai.stripPrefix')}
        hint={t('editor.ai.stripPrefixHint')}
      >
        <input
          value={asString(config.strip_prefix)}
          onChange={(e) => onChange({ ...config, strip_prefix: e.target.value })}
          placeholder="@ai"
        />
      </Field>
      <Field label={t('editor.ai.temperature')}>
        <input
          type="number"
          step="0.1"
          min="0"
          max="2"
          value={Number(config.temperature ?? 0.2)}
          onChange={(e) =>
            onChange({ ...config, temperature: Number(e.target.value) })
          }
        />
      </Field>
      <Field label={t('editor.ai.maxTokens')}>
        <input
          type="number"
          min="1"
          value={Number(config.max_tokens ?? 1024)}
          onChange={(e) =>
            onChange({ ...config, max_tokens: Number(e.target.value) })
          }
        />
      </Field>
    </>
  )
}

function ModelSelectField({
  config,
  onChange,
  connections,
  capability,
}: FormProps & { capability?: string }) {
  const { t } = usePreferences()
  const connId = asString(config.connection_id)
  const conn = connections.find((c) => c.id === connId)
  const modelsRaw = (conn?.config as Record<string, unknown> | undefined)?.models
  const models = Array.isArray(modelsRaw)
    ? modelsRaw
        .map((item) => {
          const obj = (item ?? {}) as Record<string, unknown>
          const caps = (obj.capabilities ?? {}) as Record<string, unknown>
          return {
            id: String(obj.id ?? ''),
            ok: capability
              ? capability === 'chat'
                ? caps.chat !== false
                : Boolean(caps[capability])
              : true,
          }
        })
        .filter((m) => m.id && m.ok)
    : []

  if (models.length === 0) {
    return (
      <Field label={t('editor.ai.model')} hint={t('editor.exprHint')}>
        <input
          value={asString(config.model)}
          onChange={(e) => onChange({ ...config, model: e.target.value })}
          placeholder="gpt-4o-mini"
        />
      </Field>
    )
  }

  return (
    <Field label={t('editor.ai.model')}>
      <select
        value={asString(config.model)}
        onChange={(e) => onChange({ ...config, model: e.target.value })}
      >
        <option value="">{t('settings.agent.defaultModel')}</option>
        {models.map((m) => (
          <option key={m.id} value={m.id}>
            {m.id}
          </option>
        ))}
      </select>
    </Field>
  )
}

function AiImageForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  const mode = asString(config.mode) || 'generate'
  return (
    <>
      <InfoBlock>{t('editor.ai.imageHelp')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="openai"
      />
      <Field label={t('editor.ai.mode')}>
        <select
          value={mode}
          onChange={(e) => onChange({ ...config, mode: e.target.value })}
        >
          <option value="generate">{t('editor.ai.modeGenerate')}</option>
          <option value="edit">{t('editor.ai.modeEdit')}</option>
        </select>
      </Field>
      <ModelSelectField
        config={config}
        onChange={onChange}
        connections={connections}
        capability={mode === 'edit' ? 'image_edit' : 'image_generate'}
      />
      <Field label={t('editor.ai.prompt')} hint={t('editor.exprHint')}>
        <ExpressionField
          multiline
          rows={3}
          value={asString(config.prompt)}
          onChange={(prompt) => onChange({ ...config, prompt })}
        />
      </Field>
      {mode === 'edit' && (
        <Field label={t('editor.ai.image')} hint={t('editor.ai.imageHint')}>
          <ExpressionField
            value={asString(config.image)}
            onChange={(image) => onChange({ ...config, image })}
          />
        </Field>
      )}
      <Field label={t('editor.ai.size')}>
        <input
          value={asString(config.size) || '1024x1024'}
          onChange={(e) => onChange({ ...config, size: e.target.value })}
        />
      </Field>
    </>
  )
}

function AiAudioForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  const mode = asString(config.mode) || 'tts'
  return (
    <>
      <InfoBlock>{t('editor.ai.audioHelp')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="openai"
      />
      <Field label={t('editor.ai.mode')}>
        <select
          value={mode}
          onChange={(e) => onChange({ ...config, mode: e.target.value })}
        >
          <option value="tts">{t('editor.ai.modeTts')}</option>
          <option value="transcribe">{t('editor.ai.modeTranscribe')}</option>
        </select>
      </Field>
      <ModelSelectField
        config={config}
        onChange={onChange}
        connections={connections}
        capability={mode === 'transcribe' ? 'audio_transcribe' : 'audio_generate'}
      />
      {mode === 'transcribe' ? (
        <Field label={t('editor.ai.audio')} hint={t('editor.ai.audioHint')}>
          <ExpressionField
            value={asString(config.audio)}
            onChange={(audio) => onChange({ ...config, audio })}
          />
        </Field>
      ) : (
        <>
          <Field label={t('editor.ai.text')} hint={t('editor.exprHint')}>
            <ExpressionField
              multiline
              rows={3}
              value={asString(config.text)}
              onChange={(text) => onChange({ ...config, text })}
            />
          </Field>
          <Field label={t('editor.ai.voice')}>
            <input
              value={asString(config.voice) || 'alloy'}
              onChange={(e) => onChange({ ...config, voice: e.target.value })}
            />
          </Field>
          <Field label={t('editor.ai.format')}>
            <input
              value={asString(config.format) || 'mp3'}
              onChange={(e) => onChange({ ...config, format: e.target.value })}
            />
          </Field>
        </>
      )}
    </>
  )
}

function AiVideoForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.ai.videoHelp')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="openai"
      />
      <ModelSelectField
        config={config}
        onChange={onChange}
        connections={connections}
        capability="video_generate"
      />
      <Field label={t('editor.ai.prompt')} hint={t('editor.exprHint')}>
        <ExpressionField
          multiline
          rows={3}
          value={asString(config.prompt)}
          onChange={(prompt) => onChange({ ...config, prompt })}
        />
      </Field>
      <Field label={t('editor.ai.size')}>
        <input
          value={asString(config.size)}
          onChange={(e) => onChange({ ...config, size: e.target.value })}
          placeholder="optional"
        />
      </Field>
      <Field label={t('editor.ai.seconds')}>
        <input
          type="number"
          min="1"
          value={Number(config.seconds ?? 4)}
          onChange={(e) =>
            onChange({ ...config, seconds: Number(e.target.value) })
          }
        />
      </Field>
    </>
  )
}

function labelsToText(v: unknown): string {
  if (Array.isArray(v)) {
    return v.map((item) => String(item)).join(', ')
  }
  return asString(v)
}

function AiClassifyForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.classify.help')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="openai"
      />
      <Field label={t('editor.ai.model')} hint={t('editor.exprHint')}>
        <input
          value={asString(config.model)}
          onChange={(e) => onChange({ ...config, model: e.target.value })}
          placeholder="gpt-4o-mini"
        />
      </Field>
      <Field label={t('editor.classify.text')} hint={t('editor.exprHint')}>
        <textarea
          rows={3}
          value={asString(config.text, '{{trigger.text}}')}
          onChange={(e) => onChange({ ...config, text: e.target.value })}
        />
      </Field>
      <Field label={t('editor.classify.labels')} hint={t('editor.classify.labelsHint')}>
        <input
          value={labelsToText(config.labels)}
          onChange={(e) =>
            onChange({
              ...config,
              labels: e.target.value
                .split(',')
                .map((s) => s.trim())
                .filter(Boolean),
            })
          }
        />
      </Field>
      <Field label={t('editor.ai.system')} hint={t('editor.classify.systemHint')}>
        <textarea
          rows={3}
          value={asString(config.system)}
          onChange={(e) => onChange({ ...config, system: e.target.value })}
          placeholder={t('editor.classify.systemPlaceholder')}
        />
      </Field>
    </>
  )
}

function WebSearchForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.webSearch.help')}</InfoBlock>
      <Field label={t('editor.webSearch.query')} hint={t('editor.exprHint')}>
        <textarea
          rows={3}
          value={asString(config.query)}
          onChange={(e) => onChange({ ...config, query: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webSearch.limit')}>
        <input
          type="number"
          min="1"
          max="20"
          value={asNumber(config.limit, 8)}
          onChange={(e) => onChange({ ...config, limit: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.webSearch.freshness')} hint={t('editor.webSearch.freshnessHint')}>
        <select
          value={asString(config.freshness, 'auto')}
          onChange={(e) => onChange({ ...config, freshness: e.target.value })}
        >
          <option value="auto">{t('editor.webSearch.freshness.auto')}</option>
          <option value="day">{t('editor.webSearch.freshness.day')}</option>
          <option value="week">{t('editor.webSearch.freshness.week')}</option>
          <option value="month">{t('editor.webSearch.freshness.month')}</option>
          <option value="year">{t('editor.webSearch.freshness.year')}</option>
          <option value="any">{t('editor.webSearch.freshness.any')}</option>
        </select>
      </Field>
      <Field label={t('editor.web.render')} hint={t('editor.web.renderHint')}>
        <select
          value={asString(config.render, 'auto')}
          onChange={(e) => onChange({ ...config, render: e.target.value })}
        >
          <option value="auto">{t('editor.web.render.auto')}</option>
          <option value="http">{t('editor.web.render.http')}</option>
          <option value="browser">{t('editor.web.render.browser')}</option>
        </select>
      </Field>
      <Field label={t('editor.webSearch.readPages')} hint={t('editor.webSearch.readPagesHint')}>
        <input
          type="number"
          min="0"
          max="10"
          value={asNumber(config.read_pages, 3)}
          onChange={(e) => onChange({ ...config, read_pages: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.webSearch.pageChars')}>
        <input
          type="number"
          min="200"
          max="20000"
          step="500"
          value={asNumber(config.page_chars, 3000)}
          onChange={(e) => onChange({ ...config, page_chars: Number(e.target.value) })}
        />
      </Field>
    </>
  )
}

function fieldsToText(v: unknown): string {
  if (typeof v === 'string') return v
  return Object.entries(asObject(v))
    .map(([name, selector]) => `${name}: ${asString(selector)}`)
    .join('\n')
}

function WebOpenForm({ config, onChange }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.webOpen.help')}</InfoBlock>
      <Field label={t('editor.webOpen.url')} hint={t('editor.webOpen.urlHint')}>
        <textarea
          rows={2}
          value={asString(config.url)}
          onChange={(e) => onChange({ ...config, url: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webOpen.limit')}>
        <input
          type="number"
          min="1"
          max="10"
          value={asNumber(config.limit, 5)}
          onChange={(e) => onChange({ ...config, limit: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.webOpen.maxChars')}>
        <input
          type="number"
          min="200"
          max="100000"
          step="1000"
          value={asNumber(config.max_chars, 8000)}
          onChange={(e) => onChange({ ...config, max_chars: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.webOpen.selector')} hint={t('editor.webOpen.selectorHint')}>
        <input
          value={asString(config.selector)}
          placeholder="article, .post-body"
          onChange={(e) => onChange({ ...config, selector: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webOpen.fields')} hint={t('editor.webOpen.fieldsHint')}>
        <textarea
          rows={4}
          value={fieldsToText(config.fields)}
          placeholder={'price: .price\nimages: img@src[]'}
          onChange={(e) => onChange({ ...config, fields: e.target.value })}
        />
      </Field>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.include_links !== false}
          onChange={(e) => onChange({ ...config, include_links: e.target.checked })}
        />
        {t('editor.webOpen.includeLinks')}
      </label>
      <Field label={t('editor.web.render')} hint={t('editor.web.renderHint')}>
        <select
          value={asString(config.render, 'auto')}
          onChange={(e) => onChange({ ...config, render: e.target.value })}
        >
          <option value="auto">{t('editor.web.render.auto')}</option>
          <option value="http">{t('editor.web.render.http')}</option>
          <option value="browser">{t('editor.web.render.browser')}</option>
        </select>
      </Field>
      <Field label={t('editor.webOpen.waitFor')} hint={t('editor.webOpen.waitForHint')}>
        <input
          value={asString(config.wait_for)}
          placeholder=".article-body"
          onChange={(e) => onChange({ ...config, wait_for: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webOpen.waitText')}>
        <input
          value={asString(config.wait_text)}
          onChange={(e) => onChange({ ...config, wait_text: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webOpen.waitJs')} hint={t('editor.webOpen.waitJsHint')}>
        <input
          value={asString(config.wait_js)}
          placeholder="document.querySelectorAll('.item').length > 10"
          onChange={(e) => onChange({ ...config, wait_js: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webOpen.networkIdle')} hint={t('editor.webOpen.networkIdleHint')}>
        <input
          type="number"
          min="0"
          max="10000"
          step="250"
          value={asNumber(config.network_idle_ms, 0)}
          onChange={(e) => onChange({ ...config, network_idle_ms: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.webOpen.waitMs')}>
        <input
          type="number"
          min="0"
          max="30000"
          step="250"
          value={asNumber(config.wait_ms, 500)}
          onChange={(e) => onChange({ ...config, wait_ms: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.webOpen.script')} hint={t('editor.webOpen.scriptHint')}>
        <textarea
          rows={4}
          value={asString(config.script)}
          placeholder={"[...document.querySelectorAll('.price')].map(e => e.textContent)"}
          onChange={(e) => onChange({ ...config, script: e.target.value })}
        />
      </Field>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.screenshot === true}
          onChange={(e) => onChange({ ...config, screenshot: e.target.checked })}
        />
        {t('editor.webOpen.screenshot')}
      </label>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.capture_network === true}
          onChange={(e) => onChange({ ...config, capture_network: e.target.checked })}
        />
        {t('editor.webOpen.captureNetwork')}
      </label>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.fetch_api === true}
          onChange={(e) => onChange({ ...config, fetch_api: e.target.checked })}
        />
        {t('editor.webOpen.fetchApi')}
      </label>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.structured !== false}
          onChange={(e) => onChange({ ...config, structured: e.target.checked })}
        />
        {t('editor.webOpen.structured')}
      </label>
    </>
  )
}

function WebExtractForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.webExtract.help')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="openai"
      />
      <ModelSelectField
        config={config}
        onChange={onChange}
        connections={connections}
        capability="chat"
      />
      <Field label={t('editor.webOpen.url')} hint={t('editor.webOpen.urlHint')}>
        <textarea
          rows={2}
          value={asString(config.url)}
          onChange={(e) => onChange({ ...config, url: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webExtract.instruction')} hint={t('editor.exprHint')}>
        <textarea
          rows={3}
          value={asString(config.instruction)}
          onChange={(e) => onChange({ ...config, instruction: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webExtract.schema')} hint={t('editor.webExtract.schemaHint')}>
        <textarea
          rows={4}
          value={fieldsToText(config.schema)}
          placeholder={'name: название\nprice: цена'}
          onChange={(e) => onChange({ ...config, schema: e.target.value })}
        />
      </Field>
      <Field label={t('editor.webOpen.limit')}>
        <input
          type="number"
          min="1"
          max="5"
          value={asNumber(config.limit, 3)}
          onChange={(e) => onChange({ ...config, limit: Number(e.target.value) })}
        />
      </Field>
      <Field label={t('editor.web.render')} hint={t('editor.web.renderHint')}>
        <select
          value={asString(config.render, 'auto')}
          onChange={(e) => onChange({ ...config, render: e.target.value })}
        >
          <option value="auto">{t('editor.web.render.auto')}</option>
          <option value="http">{t('editor.web.render.http')}</option>
          <option value="browser">{t('editor.web.render.browser')}</option>
        </select>
      </Field>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.capture_network === true}
          onChange={(e) => onChange({ ...config, capture_network: e.target.checked })}
        />
        {t('editor.webOpen.captureNetwork')}
      </label>
      <label className="editor__check">
        <input
          type="checkbox"
          checked={config.strict === true}
          onChange={(e) => onChange({ ...config, strict: e.target.checked })}
        />
        {t('editor.webExtract.strict')}
      </label>
    </>
  )
}

function GitHubLatestReleaseForm({ config, onChange, connections }: FormProps) {
  const { t } = usePreferences()
  return (
    <>
      <InfoBlock>{t('editor.github.help')}</InfoBlock>
      <ConnectionField
        config={config}
        onChange={onChange}
        connections={connections}
        type="github"
      />
      <Field label={t('editor.github.owner')} hint={t('editor.github.ownerHint')}>
        <input
          value={asString(config.owner)}
          onChange={(e) => onChange({ ...config, owner: e.target.value })}
          placeholder="owner"
        />
      </Field>
      <Field label={t('editor.github.repo')} hint={t('editor.github.repoHint')}>
        <input
          value={asString(config.repo)}
          onChange={(e) => onChange({ ...config, repo: e.target.value })}
          placeholder="repo"
        />
      </Field>
      <Field label={t('editor.github.trackNew')} hint={t('editor.github.trackNewHint')}>
        <select
          value={config.track_new === false ? 'false' : 'true'}
          onChange={(e) =>
            onChange({ ...config, track_new: e.target.value === 'true' })
          }
        >
          <option value="true">{t('editor.github.trackNewYes')}</option>
          <option value="false">{t('editor.github.trackNewNo')}</option>
        </select>
      </Field>
    </>
  )
}
