import { useCallback, useEffect, useMemo, useState } from 'react'
import { getLedgerSummary, listLedger } from '../api/ledger'
import { listObjectives } from '../api/objectives'
import { getWorkflows } from '../api/workflows'
import { usePreferences } from '../settings/PreferencesContext'
import type { LedgerEntry, LedgerGroup, LedgerSummary } from '../types'
import { IconRefresh, IconSpend } from '../ui/icons'
import { Modal } from '../ui/Modal'
import { formatRelative } from '../ui/format'
import { Badge, EmptyState, Notice, Segmented, Stat, Tabs } from '../ui/primitives'

type Tab = 'overview' | 'scopes' | 'calls'
type Days = '1' | '7' | '30' | '90'

const RECENT_LIMIT = 100

function errText(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}

function sinceIso(days: Days) {
  return new Date(Date.now() - Number(days) * 86_400_000).toISOString()
}

export function SpendPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t, locale } = usePreferences()
  const [tab, setTab] = useState<Tab>('overview')
  const [days, setDays] = useState<Days>('30')
  const [model, setModel] = useState('')
  const [summary, setSummary] = useState<LedgerSummary | null>(null)
  const [entries, setEntries] = useState<LedgerEntry[]>([])
  const [names, setNames] = useState<Record<string, string>>({})
  const [message, setMessage] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const usd = useCallback(
    (v: number) => {
      const digits = v !== 0 && Math.abs(v) < 1 ? 4 : 2
      return new Intl.NumberFormat(locale, {
        style: 'currency',
        currency: 'USD',
        minimumFractionDigits: digits,
        maximumFractionDigits: digits,
      }).format(v)
    },
    [locale],
  )
  const int = useCallback((v: number) => new Intl.NumberFormat(locale).format(v), [locale])

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const filter = { since: sinceIso(days), model: model || undefined }
      const [s, list] = await Promise.all([
        getLedgerSummary(filter),
        listLedger({ ...filter, limit: RECENT_LIMIT }),
      ])
      setSummary(s)
      setEntries(list)
    } catch (err) {
      setMessage(errText(err))
    } finally {
      setLoading(false)
    }
  }, [days, model])

  useEffect(() => {
    if (open) void reload()
  }, [open, reload])

  // Objective / workflow titles for the attribution tables; failures only cost
  // readability, so they are ignored.
  useEffect(() => {
    if (!open) return
    void Promise.allSettled([listObjectives(), getWorkflows()]).then(([objs, wfs]) => {
      const map: Record<string, string> = {}
      if (objs.status === 'fulfilled') for (const o of objs.value) map[o.id] = o.title
      if (wfs.status === 'fulfilled') for (const w of wfs.value) map[w.id] = w.name
      setNames(map)
    })
  }, [open])

  // A model-filtered summary only lists that one model, so the options come
  // from the last unfiltered summary to allow switching between models.
  const [knownModels, setKnownModels] = useState<string[]>([])
  useEffect(() => {
    if (summary && !model) setKnownModels(summary.by_model.map((g) => g.key))
  }, [summary, model])
  const models = useMemo(
    () => (model && !knownModels.includes(model) ? [model, ...knownModels] : knownModels),
    [knownModels, model],
  )

  const totals = summary?.totals
  const avgLatency = totals && totals.calls > 0 ? Math.round(totals.duration_ms / totals.calls) : 0

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      icon={<IconSpend size={18} />}
      title={t('spend.title')}
      subtitle={t('spend.hint')}
      actions={
        <button
          type="button"
          className="icon-btn"
          onClick={() => void reload()}
          title={t('common.refresh')}
          aria-label={t('common.refresh')}
        >
          {loading ? <span className="spinner spinner--xs" /> : <IconRefresh size={15} />}
        </button>
      }
    >
      <div className="sales">
        <Tabs<Tab>
          value={tab}
          onChange={setTab}
          items={[
            { id: 'overview', label: t('spend.tab.overview') },
            { id: 'scopes', label: t('spend.tab.scopes') },
            { id: 'calls', label: t('spend.tab.calls'), count: entries.length },
          ]}
        />
        <Notice text={message} onDismiss={() => setMessage(null)} />

        <div className="sales__section">
          <div className="toolbar">
            <span className="eyebrow">{t('spend.period')}</span>
            <Segmented<Days>
              value={days}
              onChange={setDays}
              options={[
                { value: '1', label: t('spend.days', { n: 1 }) },
                { value: '7', label: t('spend.days', { n: 7 }) },
                { value: '30', label: t('spend.days', { n: 30 }) },
                { value: '90', label: t('spend.days', { n: 90 }) },
              ]}
            />
            <select
              className="spend__model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              aria-label={t('spend.model')}
            >
              <option value="">{t('spend.allModels')}</option>
              {models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>

          {!summary || !totals ? (
            <EmptyState title={loading ? t('spend.loading') : t('spend.empty')} compact />
          ) : totals.calls === 0 ? (
            <EmptyState title={t('spend.empty')} hint={t('spend.emptyHint')} compact />
          ) : (
            <>
              {tab === 'overview' && (
                <>
                  <div className="stat-grid">
                    <Stat label={t('spend.cost')} value={usd(totals.cost_usd)} tone="warning" />
                    <Stat
                      label={t('spend.calls')}
                      value={int(totals.calls)}
                      hint={
                        totals.failed_calls > 0 ? t('spend.failedN', { n: totals.failed_calls }) : undefined
                      }
                      tone={totals.failed_calls > 0 ? 'error' : undefined}
                    />
                    <Stat
                      label={t('spend.tokens')}
                      value={int(totals.input_tokens + totals.output_tokens)}
                      hint={`${int(totals.input_tokens)} ↓ · ${int(totals.output_tokens)} ↑`}
                    />
                    <Stat label={t('spend.avgLatency')} value={`${int(avgLatency)} ms`} />
                    {totals.unpriced_calls > 0 && (
                      <Stat
                        label={t('spend.unpriced')}
                        value={int(totals.unpriced_calls)}
                        hint={t('spend.unpricedHint')}
                      />
                    )}
                  </div>
                  <div className="sales__grid">
                    <section className="sales__card">
                      <h4 className="sales__card-title">{t('spend.byDay')}</h4>
                      <SpendChart
                        days={fillDays(summary.by_day, Number(days))}
                        usd={usd}
                        empty={t('spend.noCost')}
                      />
                    </section>
                  </div>
                  <GroupTable
                    title={t('spend.byModel')}
                    keyLabel={t('spend.model')}
                    rows={summary.by_model}
                    usd={usd}
                    int={int}
                    onPick={(k) => setModel(k)}
                  />
                </>
              )}

              {tab === 'scopes' && (
                <>
                  <GroupTable
                    title={t('spend.byObjective')}
                    keyLabel={t('spend.objective')}
                    rows={summary.by_objective}
                    label={(k) => names[k] ?? k.slice(0, 8)}
                    usd={usd}
                    int={int}
                    empty={t('spend.noScope')}
                  />
                  <GroupTable
                    title={t('spend.byWorkflow')}
                    keyLabel={t('spend.workflow')}
                    rows={summary.by_workflow}
                    label={(k) => names[k] ?? k.slice(0, 8)}
                    usd={usd}
                    int={int}
                    empty={t('spend.noScope')}
                  />
                </>
              )}

              {tab === 'calls' && (
                <div className="table-wrap">
                  <table className="table">
                    <thead>
                      <tr>
                        <th>{t('spend.when')}</th>
                        <th>{t('spend.model')}</th>
                        <th>{t('spend.scope')}</th>
                        <th className="num">{t('spend.tokens')}</th>
                        <th className="num">{t('spend.cost')}</th>
                        <th className="num">{t('spend.duration')}</th>
                        <th>{t('spend.status')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {entries.map((e) => {
                        const scope = e.objective_id ?? e.workflow_id
                        return (
                          <tr key={e.id}>
                            <td title={new Date(e.at).toLocaleString(locale)}>{formatRelative(e.at, locale)}</td>
                            <td>{e.model}</td>
                            <td className="sales__muted">
                              {scope ? names[scope] ?? scope.slice(0, 8) : '—'}
                              {e.item_id ? ` · ${e.item_id}` : ''}
                            </td>
                            <td className="num">
                              {int(e.input_tokens)} / {int(e.output_tokens)}
                            </td>
                            <td className="num">{e.cost_usd == null ? '—' : usd(e.cost_usd)}</td>
                            <td className="num">{int(e.duration_ms)} ms</td>
                            <td>
                              {e.ok ? (
                                <Badge tone="success">{t('spend.ok')}</Badge>
                              ) : (
                                <Badge tone="error" title={e.error}>
                                  {t('spend.failed')}
                                </Badge>
                              )}
                            </td>
                          </tr>
                        )
                      })}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </Modal>
  )
}

const ZERO_TOTALS = {
  calls: 0,
  failed_calls: 0,
  input_tokens: 0,
  output_tokens: 0,
  cost_usd: 0,
  unpriced_calls: 0,
  duration_ms: 0,
}

// `by_day` only has days with calls; pad the period with empty UTC days so
// the bars keep a real time axis.
function fillDays(days: LedgerGroup[], span: number): LedgerGroup[] {
  const byKey = new Map(days.map((d) => [d.key, d]))
  const out: LedgerGroup[] = []
  for (let i = span; i >= 0; i--) {
    const key = new Date(Date.now() - i * 86_400_000).toISOString().slice(0, 10)
    out.push(byKey.get(key) ?? { key, ...ZERO_TOTALS })
  }
  return out
}

function SpendChart({
  days,
  usd,
  empty,
}: {
  days: LedgerGroup[]
  usd: (v: number) => string
  empty: string
}) {
  const max = Math.max(0, ...days.map((d) => d.cost_usd))
  if (max <= 0) return <p className="sales__muted">{empty}</p>
  return (
    <div className="sales-chart" role="img">
      {days.map((d) => (
        <span
          key={d.key}
          className="sales-chart__bar"
          style={{ height: `${Math.max(2, (d.cost_usd / max) * 100)}%` }}
          title={`${d.key}: ${usd(d.cost_usd)} · ${d.calls}`}
        />
      ))}
    </div>
  )
}

function GroupTable({
  title,
  keyLabel,
  rows,
  label = (k) => k,
  usd,
  int,
  empty,
  onPick,
}: {
  title: string
  keyLabel: string
  rows: LedgerGroup[]
  label?: (key: string) => string
  usd: (v: number) => string
  int: (v: number) => string
  empty?: string
  onPick?: (key: string) => void
}) {
  const { t } = usePreferences()
  const total = rows.reduce((s, r) => s + r.cost_usd, 0)
  return (
    <section className="sales__card">
      <h4 className="sales__card-title">{title}</h4>
      {rows.length === 0 ? (
        <p className="sales__muted">{empty}</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>{keyLabel}</th>
                <th className="num">{t('spend.calls')}</th>
                <th className="num">{t('spend.tokens')}</th>
                <th className="num">{t('spend.cost')}</th>
                <th className="num">%</th>
                <th className="num">{t('spend.failed')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.key}>
                  <td title={r.key}>
                    {onPick ? (
                      <button type="button" className="link-btn" onClick={() => onPick(r.key)}>
                        {label(r.key)}
                      </button>
                    ) : (
                      label(r.key)
                    )}
                  </td>
                  <td className="num">{int(r.calls)}</td>
                  <td className="num">{int(r.input_tokens + r.output_tokens)}</td>
                  <td className="num">{usd(r.cost_usd)}</td>
                  <td className="num">{total > 0 ? Math.round((r.cost_usd / total) * 100) : 0}</td>
                  <td className="num">{r.failed_calls > 0 ? int(r.failed_calls) : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}
