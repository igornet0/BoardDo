import { useEffect, useMemo, useState } from 'react'
import {
  approveAction,
  createCampaign,
  createGoal,
  deleteGoal,
  getCampaignPlan,
  getGoal,
  getMarketingSnapshot,
  getContentEngineSnapshot,
  listAgentTemplates,
  listApprovals,
  listExperiments,
  listGoalActivity,
  listGoalMemory,
  listGoals,
  pauseGoal,
  recordGoalEvent,
  rejectAction,
  startGoal,
  stopGoal,
  goalsWsUrl,
} from '../api/goals'
import { usePreferences } from '../settings/PreferencesContext'
import { Modal, useConfirm } from '../ui/Modal'
import { Badge, Card, EmptyState, Field, Notice, Progress, Stat, Tabs, type Tone } from '../ui/primitives'
import { formatDateTime, formatRelative, formatTime } from '../ui/format'
import {
  IconActivity,
  IconCheck,
  IconClock,
  IconExternal,
  IconLightning,
  IconPause,
  IconPlay,
  IconPlus,
  IconSparkle,
  IconStop,
  IconTarget,
  IconTrash,
  IconUser,
} from '../ui/icons'
import type { MessageKey } from '../i18n/messages'
import type {
  AgentApproval,
  AgentAuditEntry,
  AgentMemoryEntry,
  AgentTemplate,
  CampaignPlan,
  Experiment,
  GoalDetail,
  GoalRunStatus,
  MarketingSnapshot,
  ContentEngineSnapshot,
} from '../types'

interface Props {
  open: boolean
  onClose: () => void
  onOpenWorkflow?: (id: string) => void
}

function barWidth(value: unknown): string {
  const n = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(n)) return '0%'
  return `${Math.round(Math.min(1, Math.max(0, n)) * 100)}%`
}

function deadlineLeft(iso?: string | null): string {
  if (!iso) return '—'
  const ms = new Date(iso).getTime() - Date.now()
  if (Number.isNaN(ms)) return '—'
  if (ms <= 0) return '0'
  const d = Math.floor(ms / 86400000)
  const h = Math.floor((ms % 86400000) / 3600000)
  return `${d}d ${h}h`
}

function statusTone(status?: GoalRunStatus): Tone {
  if (status === 'running') return 'success'
  if (status === 'succeeded') return 'info'
  if (status === 'waiting_approval') return 'warning'
  if (status === 'paused') return 'warning'
  if (status === 'failed') return 'error'
  return 'neutral'
}

const TABS = [
  'plan',
  'activity',
  'funnel',
  'content',
  'strategy',
  'experiments',
  'learnings',
  'memory',
  'approvals',
  'policy',
  'scenarios',
] as const

export function GoalsPanel({ open, onClose, onOpenWorkflow }: Props) {
  const { t, locale } = usePreferences()
  const confirm = useConfirm()
  const [items, setItems] = useState<GoalDetail[]>([])
  const [templates, setTemplates] = useState<AgentTemplate[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [activity, setActivity] = useState<AgentAuditEntry[]>([])
  const [experiments, setExperiments] = useState<Experiment[]>([])
  const [memory, setMemory] = useState<AgentMemoryEntry[]>([])
  const [approvals, setApprovals] = useState<AgentApproval[]>([])
  const [marketing, setMarketing] = useState<MarketingSnapshot | null>(null)
  const [contentEngine, setContentEngine] = useState<ContentEngineSnapshot | null>(
    null,
  )
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [showWizard, setShowWizard] = useState(false)
  const [plan, setPlan] = useState<CampaignPlan | null>(null)
  const [wizard, setWizard] = useState({
    product_name: '',
    product_description: '',
    product_url: '',
    target: '1000',
    budget_usd: '300',
    market: 'Germany',
    deadline_days: '30',
    autonomy: 'assisted',
    demo: true,
  })
  const [tab, setTab] = useState<
    | 'plan'
    | 'activity'
    | 'funnel'
    | 'content'
    | 'strategy'
    | 'experiments'
    | 'learnings'
    | 'memory'
    | 'approvals'
    | 'policy'
    | 'scenarios'
  >('plan')

  const selected =
    items.find((g) => g.spec.id === selectedId) ?? items[0] ?? null

  async function refreshList() {
    try {
      const [goals, tpl] = await Promise.all([
        listGoals(),
        listAgentTemplates(),
      ])
      setItems(goals)
      setTemplates(tpl.templates)
      if (!selectedId && goals[0]) setSelectedId(goals[0].spec.id)
    } catch (err) {
      setMessage(String(err))
    }
  }

  async function refreshDetail(id: string, runId?: string | null) {
    try {
      const detail = await getGoal(id)
      setItems((prev) => {
        const rest = prev.filter((g) => g.spec.id !== id)
        return [detail, ...rest]
      })
      const rid = runId ?? detail.run?.id
      if (!rid) {
        setActivity([])
        setExperiments([])
        setMemory([])
        setApprovals([])
        setMarketing(null)
        setContentEngine(null)
        return
      }
      const [act, exp, mem, appr, mkt, content] = await Promise.all([
        listGoalActivity(rid),
        listExperiments(rid),
        listGoalMemory(rid),
        listApprovals(rid, false),
        detail.spec.agent_type_id === 'agent.marketing'
          ? getMarketingSnapshot(rid).catch(() => null)
          : Promise.resolve(null),
        ['agent.marketing', 'agent.content'].includes(detail.spec.agent_type_id)
          ? getContentEngineSnapshot(rid).catch(() => null)
          : Promise.resolve(null),
      ])
      setActivity(act.entries)
      setExperiments(exp.experiments)
      setMemory(mem.memory)
      setApprovals(appr)
      setMarketing(mkt)
      setContentEngine(content)
      if (mkt?.plan) {
        setPlan(mkt.plan)
      } else if (detail.spec.agent_type_id === 'agent.marketing' && rid) {
        try {
          const p = await getCampaignPlan(detail.spec.id)
          setPlan(p)
        } catch {
          setPlan(null)
        }
      } else {
        setPlan(null)
      }
      if (detail.run?.status === 'draft') {
        setTab('plan')
      }
    } catch (err) {
      setMessage(String(err))
    }
  }

  useEffect(() => {
    if (!open) return
    void refreshList()
  }, [open])

  useEffect(() => {
    if (!open || !selected?.spec.id) return
    void refreshDetail(selected.spec.id, selected.run?.id)
    const ws = new WebSocket(goalsWsUrl())
    ws.onmessage = () => {
      void refreshDetail(selected.spec.id, selected.run?.id)
      void refreshList()
    }
    return () => ws.close()
  }, [open, selected?.spec.id, selected?.run?.id])

  const progress = useMemo(() => {
    if (!selected) return 0
    const cur = selected.run?.current ?? 0
    const tgt = selected.spec.target || 1
    return Math.min(100, Math.round((cur / tgt) * 100))
  }, [selected])

  function statusLabel(status?: GoalRunStatus) {
    return t(`goals.status.${status ?? 'draft'}` as MessageKey)
  }

  async function withBusy(fn: () => Promise<void>) {
    setBusy(true)
    setMessage(null)
    try {
      await fn()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function createFromTemplate(tpl: AgentTemplate) {
    await withBusy(async () => {
      const spec = await createGoal({
        title: tpl.title,
        text:
          tpl.id === 'growth'
            ? t('goals.growthText')
            : tpl.id === 'marketing'
              ? t('goals.marketingText')
              : tpl.description,
        template_id: tpl.id,
        agent_type_id: tpl.agent_type_id,
        metric: tpl.metric,
        target: tpl.target,
        tick_interval_secs: 30,
        constraints:
          tpl.id === 'marketing'
            ? {
                product: 'BoardDo',
                budget: 0,
                target_leads: 10,
                channel: 'telegram',
                autonomy_level: 2,
                demo: true,
              }
            : undefined,
      })
      setShowWizard(false)
      await refreshList()
      setSelectedId(spec.id)
      setMessage(t('goals.created'))
    })
  }

  async function submitWizard() {
    await withBusy(async () => {
      const created = await createCampaign({
        product_name: wizard.product_name.trim(),
        product_description: wizard.product_description,
        product_url: wizard.product_url || null,
        target: Number(wizard.target) || 1000,
        budget_usd: Number(wizard.budget_usd) || 0,
        market: wizard.market,
        deadline_days: Number(wizard.deadline_days) || 30,
        autonomy: wizard.autonomy,
        demo: wizard.demo,
        goal_metric: 'leads_interested',
      })
      setPlan(created.plan)
      setShowWizard(false)
      await refreshList()
      setSelectedId(created.spec.id)
      setTab('plan')
      setMessage(t('goals.planReady'))
    })
  }

  async function start(id: string) {
    await withBusy(async () => {
      await startGoal(id)
      await refreshDetail(id)
      setTab('activity')
    })
  }

  async function remove(goal: GoalDetail) {
    const ok = await confirm({
      title: t('goals.delete'),
      message: t('goals.confirmDelete', { name: goal.spec.title }),
      confirmLabel: t('goals.delete'),
      danger: true,
    })
    if (!ok) return
    await withBusy(async () => {
      await deleteGoal(goal.spec.id)
      setSelectedId(null)
      setPlan(null)
      await refreshList()
    })
  }

  const pendingApprovals = approvals.filter((a) => a.status === 'pending').length
  const status = selected?.run?.status
  const isRunning = status === 'running' || status === 'waiting_approval'

  const tabCounts: Partial<Record<(typeof TABS)[number], number>> = {
    activity: activity.length,
    experiments: experiments.length,
    memory: memory.length,
    approvals: approvals.length,
    scenarios: selected?.linked_workflows?.length ?? 0,
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      flush
      icon={<IconTarget size={18} />}
      title={t('goals.title')}
      subtitle={t('goals.hint')}
    >
      <div className="mission">
        <aside className="mission__sidebar">
          <button
            type="button"
            className={`btn btn--primary btn--block${showWizard ? ' is-pressed' : ''}`}
            disabled={busy}
            onClick={() => setShowWizard(true)}
          >
            <IconPlus size={14} />
            {t('goals.newCampaign')}
          </button>

          <span className="eyebrow">{t('goals.runs')}</span>
          {items.length === 0 && <p className="mission__empty">{t('goals.empty')}</p>}
          <ul className="mission__list">
            {items.map((item) => {
              const pct = Math.min(100, Math.round(((item.run?.current ?? 0) / (item.spec.target || 1)) * 100))
              const active = !showWizard && selected?.spec.id === item.spec.id
              return (
                <li key={item.spec.id}>
                  <button
                    type="button"
                    className={`mission-item${active ? ' is-active' : ''}`}
                    onClick={() => {
                      setShowWizard(false)
                      setSelectedId(item.spec.id)
                    }}
                  >
                    <span className="mission-item__top">
                      <strong>{item.spec.title}</strong>
                      <Badge tone={statusTone(item.run?.status)} dot>
                        {statusLabel(item.run?.status)}
                      </Badge>
                    </span>
                    <Progress value={pct} />
                    <span className="mission-item__meta">
                      {Math.round(item.run?.current ?? 0)} / {item.spec.target} {item.spec.metric}
                    </span>
                  </button>
                </li>
              )
            })}
          </ul>

          {templates.length > 0 && (
            <>
              <span className="eyebrow">{t('goals.templates')}</span>
              <ul className="mission__list">
                {templates.map((tpl) => (
                  <li key={tpl.id}>
                    <button type="button" className="tpl-item" disabled={busy} onClick={() => void createFromTemplate(tpl)}>
                      <span className="tpl-item__icon">
                        <IconSparkle size={13} />
                      </span>
                      <span className="tpl-item__text">
                        <strong>{tpl.title}</strong>
                        <span>{tpl.description}</span>
                      </span>
                      <IconPlus size={13} className="tpl-item__add" />
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
        </aside>

        <section className="mission__main">
          <Notice text={message} onDismiss={() => setMessage(null)} />

          {showWizard ? (
            <Card title={t('goals.newCampaign')} subtitle={t('goals.wizardHint')}>
              <form
                className="form"
                onSubmit={(e) => {
                  e.preventDefault()
                  void submitWizard()
                }}
              >
                <div className="form__row">
                  <Field label={t('goals.productName')}>
                    <input
                      value={wizard.product_name}
                      onChange={(e) => setWizard((w) => ({ ...w, product_name: e.target.value }))}
                      placeholder="Finance tracker app"
                      required
                      autoFocus
                    />
                  </Field>
                  <Field label={t('goals.productUrl')}>
                    <input
                      value={wizard.product_url}
                      onChange={(e) => setWizard((w) => ({ ...w, product_url: e.target.value }))}
                      placeholder="https://"
                    />
                  </Field>
                </div>
                <Field label={t('goals.productDesc')}>
                  <textarea
                    value={wizard.product_description}
                    onChange={(e) => setWizard((w) => ({ ...w, product_description: e.target.value }))}
                    rows={3}
                  />
                </Field>
                <div className="form__row form__row--4">
                  <Field label={t('goals.target')}>
                    <input inputMode="numeric" value={wizard.target} onChange={(e) => setWizard((w) => ({ ...w, target: e.target.value }))} />
                  </Field>
                  <Field label={t('goals.budget')}>
                    <input inputMode="numeric" value={wizard.budget_usd} onChange={(e) => setWizard((w) => ({ ...w, budget_usd: e.target.value }))} />
                  </Field>
                  <Field label={t('goals.market')}>
                    <input value={wizard.market} onChange={(e) => setWizard((w) => ({ ...w, market: e.target.value }))} />
                  </Field>
                  <Field label={t('goals.deadlineDays')}>
                    <input inputMode="numeric" value={wizard.deadline_days} onChange={(e) => setWizard((w) => ({ ...w, deadline_days: e.target.value }))} />
                  </Field>
                </div>
                <Field label={t('goals.autonomy')}>
                  <div className="choice-cards">
                    {(
                      [
                        ['assisted', 'goals.autonomy.assisted'],
                        ['autonomous', 'goals.autonomy.autonomous'],
                        ['full_autonomous', 'goals.autonomy.full'],
                      ] as const
                    ).map(([value, key]) => (
                      <button
                        key={value}
                        type="button"
                        className={`choice-card${wizard.autonomy === value ? ' is-active' : ''}`}
                        onClick={() => setWizard((w) => ({ ...w, autonomy: value }))}
                      >
                        {t(key)}
                      </button>
                    ))}
                  </div>
                </Field>
                <label className="check">
                  <input type="checkbox" checked={wizard.demo} onChange={(e) => setWizard((w) => ({ ...w, demo: e.target.checked }))} />
                  {t('goals.demoMode')}
                </label>
                <div className="form__actions">
                  <div className="toolbar__spacer" />
                  <button type="button" className="btn btn--ghost" onClick={() => setShowWizard(false)}>
                    {t('goals.cancel')}
                  </button>
                  <button type="submit" className="btn btn--primary" disabled={busy || !wizard.product_name.trim()}>
                    <IconSparkle size={14} />
                    {t('goals.createCampaign')}
                  </button>
                </div>
              </form>
            </Card>
          ) : !selected ? (
            <EmptyState
              icon={<IconTarget size={24} />}
              title={t('goals.pick')}
              hint={t('goals.emptyHint')}
              action={
                <button type="button" className="btn btn--primary" onClick={() => setShowWizard(true)}>
                  <IconPlus size={14} />
                  {t('goals.newCampaign')}
                </button>
              }
            />
          ) : (
            <>
              <div className="mission-hero">
                <div className="mission-hero__top">
                  <span className="eyebrow">
                    {selected.spec.agent_type_id === 'agent.marketing' ? t('goals.campaign') : selected.spec.agent_type_id}
                  </span>
                  <Badge tone={statusTone(status)} dot>
                    {status === 'running' ? t('goals.agentRunning') : statusLabel(status)}
                  </Badge>
                </div>
                <h3>{selected.spec.title}</h3>
                {selected.spec.text && <p className="mission-hero__text">{selected.spec.text}</p>}

                {(marketing?.current_objective || selected.run?.last_think) && (
                  <div className="callout callout--ai">
                    <IconSparkle size={15} />
                    <div>
                      <strong>{t('goals.currentObjective')}</strong>
                      <span>{marketing?.current_objective || selected.run?.last_think}</span>
                    </div>
                  </div>
                )}

                <div className="stat-grid">
                  <Stat
                    icon={<IconTarget size={14} />}
                    label={t('goals.progress')}
                    value={`${progress}%`}
                    hint={`${Math.round(selected.run?.current ?? 0)} / ${selected.spec.target} ${selected.spec.metric}`}
                    tone="info"
                  />
                  <Stat icon={<IconClock size={14} />} label={t('goals.deadline')} value={deadlineLeft(selected.spec.deadline)} hint={selected.spec.deadline ? formatDateTime(selected.spec.deadline, locale) : undefined} />
                  <Stat
                    icon={<IconLightning size={14} />}
                    label={t('goals.actionsToday')}
                    value={`${selected.run?.actions_today ?? 0}`}
                    hint={`/ ${selected.spec.policy.max_actions_per_day}`}
                  />
                  <Stat
                    icon={<IconCheck size={14} />}
                    label={t('goals.tab.approvals')}
                    value={pendingApprovals}
                    hint={t('goals.pending')}
                    tone={pendingApprovals > 0 ? 'warning' : 'neutral'}
                  />
                </div>
                <Progress value={progress} />

                {marketing && (
                  <div className="kv-chips">
                    <span>
                      {t('goals.funnel.leads')} <b>{marketing.funnel.leads}</b>
                    </span>
                    <span>
                      {t('goals.funnel.qualified')} <b>{marketing.funnel.qualified}</b>
                    </span>
                    <span>
                      {t('goals.funnel.published')} <b>{marketing.funnel.published}</b>
                    </span>
                    <span>
                      {t('goals.autonomy')} <b>{marketing.autonomy_mode || `L${marketing.autonomy_level}`}</b>
                    </span>
                    {marketing.channel_weights &&
                      Object.entries(marketing.channel_weights)
                        .filter(([k]) => ['telegram', 'content', 'research', 'engage'].includes(k))
                        .map(([ch, w]) => (
                          <span key={ch} className="kv-chips__channel">
                            {ch} <b>{typeof w === 'number' ? `${Math.round(w * 100)}%` : ''}</b>
                          </span>
                        ))}
                  </div>
                )}

                <div className="mission-hero__actions">
                  <button type="button" className="btn btn--primary" disabled={busy || isRunning} onClick={() => void start(selected.spec.id)}>
                    <IconPlay size={13} />
                    {status === 'draft' || !status ? t('goals.startCampaign') : t('goals.start')}
                  </button>
                  <button
                    type="button"
                    className="btn"
                    disabled={busy || status !== 'running'}
                    onClick={() =>
                      void withBusy(async () => {
                        await pauseGoal(selected.spec.id)
                        await refreshDetail(selected.spec.id)
                      })
                    }
                  >
                    <IconPause size={13} />
                    {t('goals.pause')}
                  </button>
                  <button
                    type="button"
                    className="btn"
                    disabled={busy || !selected.run}
                    onClick={() =>
                      void withBusy(async () => {
                        await stopGoal(selected.spec.id)
                        await refreshDetail(selected.spec.id)
                      })
                    }
                  >
                    <IconStop size={11} />
                    {t('goals.stop')}
                  </button>
                  <button
                    type="button"
                    className="btn btn--ghost"
                    disabled={busy || !selected.run}
                    onClick={() =>
                      void (async () => {
                        try {
                          await recordGoalEvent(selected.spec.id, 'lead.interested')
                          await refreshDetail(selected.spec.id)
                        } catch (err) {
                          setMessage(String(err))
                        }
                      })()
                    }
                  >
                    <IconUser size={13} />
                    {t('goals.recordLead')}
                  </button>
                  <div className="toolbar__spacer" />
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    disabled={busy}
                    onClick={() => void remove(selected)}
                    aria-label={t('goals.delete')}
                    data-tip={t('goals.delete')}
                  >
                    <IconTrash size={15} />
                  </button>
                </div>
              </div>

              <Tabs
                value={tab}
                onChange={setTab}
                items={TABS.map((id) => ({
                  id,
                  label: t(`goals.tab.${id}` as MessageKey),
                  count: tabCounts[id] || undefined,
                  alert: id === 'approvals' && pendingApprovals > 0,
                }))}
              />

              <div className="mission__tab">
                {tab === 'plan' &&
                  (!plan ? (
                    <EmptyState compact title={t('goals.noPlan')} />
                  ) : (
                    <>
                      <p className="lead">{plan.summary}</p>
                      <div className="metric-grid">
                        {(
                          [
                            ['goals.plan.competitors', plan.competitors.length],
                            ['goals.plan.segments', plan.segments.length],
                            ['goals.plan.pains', plan.pain_points.length],
                            ['goals.plan.contentOps', plan.content_opportunities.length],
                            ['goals.plan.experiments', plan.experiments.length],
                            ['goals.plan.tasks', plan.content_tasks.length],
                          ] as const
                        ).map(([key, val]) => (
                          <div key={key} className="metric">
                            <strong>{val}</strong>
                            <span>{t(key)}</span>
                          </div>
                        ))}
                      </div>
                      <dl className="dl">
                        <dt>{t('goals.plan.channels')}</dt>
                        <dd>{plan.acquisition_channels.join(', ') || '—'}</dd>
                        <dt>{t('goals.plan.workload')}</dt>
                        <dd>{plan.estimated_workload}</dd>
                        <dt>{t('goals.plan.strategy')}</dt>
                        <dd>{plan.strategy_outline}</dd>
                      </dl>
                      {status === 'draft' && plan.ready && (
                        <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void start(selected.spec.id)}>
                          <IconPlay size={13} />
                          {t('goals.startCampaign')}
                        </button>
                      )}
                    </>
                  ))}

                {tab === 'activity' && (
                  <>
                    {selected.run?.last_think && (
                      <div className="callout callout--ai">
                        <IconSparkle size={15} />
                        <div>
                          <strong>{t('goals.thinking')}</strong>
                          <span>{selected.run.last_think}</span>
                        </div>
                      </div>
                    )}
                    {activity.length === 0 ? (
                      <EmptyState compact icon={<IconActivity size={18} />} title={t('goals.noActivity')} />
                    ) : (
                      <ol className="timeline">
                        {activity.map((row) => (
                          <li key={row.id}>
                            <span className="timeline__dot" />
                            <div className="timeline__body">
                              <div className="timeline__head">
                                <code>{row.kind}</code>
                                {row.at && <time title={formatDateTime(row.at, locale)}>{formatTime(row.at, locale)}</time>}
                              </div>
                              {row.message && <p>{row.message}</p>}
                            </div>
                          </li>
                        ))}
                      </ol>
                    )}
                  </>
                )}

                {tab === 'funnel' &&
                  (!marketing ? (
                    <EmptyState compact title={t('goals.noMarketing')} />
                  ) : (
                    <>
                      <div className="kv-chips">
                        <span>
                          {t('goals.stage')} <b>{marketing.stage}</b>
                        </span>
                        <span>
                          {t('goals.autonomy')} <b>L{marketing.autonomy_level}</b>
                        </span>
                      </div>
                      <span className="eyebrow">{t('goals.funnelStages')}</span>
                      <div className="stage-row">
                        {Object.entries(marketing.stage_progress).map(([stage, st]) => (
                          <span key={stage} className={`stage stage--${String(st)}`}>
                            {stage}
                            <small>{String(st)}</small>
                          </span>
                        ))}
                      </div>
                      <div className="funnel">
                        {(() => {
                          const rows = [
                            ['research_notes', marketing.funnel.research_notes],
                            ['audience_notes', marketing.funnel.audience_notes],
                            ['pain_notes', marketing.funnel.pain_notes],
                            ['hypotheses', marketing.funnel.hypotheses],
                            ['drafts', marketing.funnel.drafts],
                            ['published', marketing.funnel.published],
                            ['conversations', marketing.funnel.conversations],
                            ['leads', marketing.funnel.leads],
                            ['qualified', marketing.funnel.qualified],
                            ['conversions', marketing.funnel.conversions],
                          ] as const
                          const max = Math.max(1, ...rows.map(([, v]) => Number(v) || 0))
                          return rows.map(([key, val]) => (
                            <div key={key} className="funnel__row">
                              <span className="funnel__label">{t(`goals.funnel.${key}` as MessageKey)}</span>
                              <span className="funnel__bar">
                                <i style={{ width: `${((Number(val) || 0) / max) * 100}%` }} />
                              </span>
                              <b>{val}</b>
                            </div>
                          ))
                        })()}
                      </div>
                      {marketing.strategy_deltas[0] && (
                        <div className="callout callout--info">
                          <IconActivity size={15} />
                          <div>
                            <strong>{t('goals.latestDelta')}</strong>
                            <span>
                              {marketing.strategy_deltas[0].observation} → {marketing.strategy_deltas[0].action} (
                              {Math.round(marketing.strategy_deltas[0].confidence * 100)}%)
                            </span>
                          </div>
                        </div>
                      )}
                      {marketing.leads.length > 0 && (
                        <ul className="row-list">
                          {marketing.leads.map((lead) => (
                            <li key={lead.id} className="row">
                              <span className="row__icon">
                                <IconUser size={14} />
                              </span>
                              <div className="row__main">
                                <div className="row__title">
                                  <strong>{lead.stage}</strong>
                                  <Badge tone="info">score {lead.score}</Badge>
                                  {lead.simulated && <Badge tone="warning">SIMULATED</Badge>}
                                </div>
                                <div className="row__meta">{lead.score_reasons.join(', ')}</div>
                              </div>
                            </li>
                          ))}
                        </ul>
                      )}
                    </>
                  ))}

                {tab === 'content' &&
                  (!contentEngine ? (
                    <EmptyState compact title={t('goals.noContent')} />
                  ) : (
                    <>
                      <p className="lead">
                        {contentEngine.project?.name ?? 'Content project'} · {contentEngine.items.length} items · {contentEngine.ideas.length} ideas
                      </p>
                      <ul className="row-list">
                        {contentEngine.items.map((item) => (
                          <li key={item.id} className="row">
                            <div className="row__main">
                              <div className="row__title">
                                <strong>{item.title}</strong>
                                <Badge tone="info">{item.lifecycle}</Badge>
                                <span className="chip">{item.channel}</span>
                              </div>
                              <div className="row__meta">
                                {item.format}
                                {item.parent_content_id ? ` ← ${item.parent_content_id.slice(0, 8)}` : ''}
                              </div>
                            </div>
                          </li>
                        ))}
                      </ul>
                      {contentEngine.graph_edges.length > 0 && (
                        <>
                          <span className="eyebrow">{t('goals.contentGraph')}</span>
                          <ul className="row-list row-list--dense">
                            {contentEngine.graph_edges.map((e) => (
                              <li key={`${e.parent_id}-${e.child_id}`} className="row">
                                <code>
                                  {e.parent_id.slice(0, 8)} → {e.child_id.slice(0, 8)}
                                </code>
                                <span className="row__meta">{e.relation}</span>
                              </li>
                            ))}
                          </ul>
                        </>
                      )}
                    </>
                  ))}

                {tab === 'strategy' &&
                  (Object.keys(selected.run?.strategy ?? {}).length === 0 ? (
                    <EmptyState compact title={t('goals.noStrategy')} />
                  ) : (
                    <div className="funnel">
                      {Object.entries(selected.run?.strategy ?? {}).map(([key, val]) => (
                        <div key={key} className="funnel__row">
                          <span className="funnel__label">{key}</span>
                          <span className="funnel__bar">
                            <i style={{ width: barWidth(val) }} />
                          </span>
                          <b>{barWidth(val)}</b>
                        </div>
                      ))}
                    </div>
                  ))}

                {tab === 'experiments' &&
                  (experiments.length === 0 ? (
                    <EmptyState compact title={t('goals.noExperiments')} />
                  ) : (
                    <ul className="row-list">
                      {experiments.map((exp) => (
                        <li key={exp.id} className="row row--stack">
                          <div className="row__title">
                            <strong>H{exp.number}</strong>
                            <Badge tone={exp.decision === 'continue' ? 'success' : exp.decision === 'stop' ? 'error' : exp.decision === 'pivot' ? 'warning' : 'neutral'}>
                              {exp.decision}
                            </Badge>
                          </div>
                          <p className="row__text">{exp.hypothesis}</p>
                          <div className="kv-chips kv-chips--sm">
                            <span>
                              posts <b>{String(exp.metrics.posts ?? 0)}</b>
                            </span>
                            <span>
                              leads <b>{String(exp.metrics.leads ?? 0)}</b>
                            </span>
                            <span>
                              qualified <b>{String(exp.metrics.qualified_leads ?? 0)}</b>
                            </span>
                            <span>
                              replies <b>{String(exp.metrics.replies ?? 0)}</b>
                            </span>
                          </div>
                        </li>
                      ))}
                    </ul>
                  ))}

                {tab === 'learnings' &&
                  (!marketing?.learnings || marketing.learnings.length === 0 ? (
                    <EmptyState compact title={t('goals.noLearnings')} />
                  ) : (
                    <ul className="row-list">
                      {marketing.learnings.map((l) => (
                        <li key={l.id} className="row row--stack">
                          <div className="row__title">
                            <span className="row__meta">{formatRelative(l.at, locale)}</span>
                            <Badge tone="ai">{Math.round(l.confidence * 100)}%</Badge>
                          </div>
                          <dl className="dl dl--compact">
                            <dt>{t('goals.learning.observation')}</dt>
                            <dd>{l.observation}</dd>
                            <dt>{t('goals.learning.decision')}</dt>
                            <dd>{l.decision}</dd>
                            <dt>{t('goals.learning.reason')}</dt>
                            <dd>{l.reason}</dd>
                          </dl>
                        </li>
                      ))}
                    </ul>
                  ))}

                {tab === 'memory' &&
                  (memory.length === 0 ? (
                    <EmptyState compact title={t('goals.noMemory')} />
                  ) : (
                    <ul className="row-list">
                      {memory.map((m) => (
                        <li key={m.id} className="row row--stack">
                          <div className="row__title">
                            <strong className="mono">{m.key}</strong>
                            <span className="chip">{m.kind}</span>
                          </div>
                          <pre className="code-inline">{JSON.stringify(m.value, null, 2)}</pre>
                        </li>
                      ))}
                    </ul>
                  ))}

                {tab === 'approvals' &&
                  (approvals.length === 0 ? (
                    <EmptyState compact icon={<IconCheck size={18} />} title={t('goals.noApprovals')} />
                  ) : (
                    <ul className="row-list">
                      {approvals.map((a) => (
                        <li key={a.id} className={`row${a.status === 'pending' ? ' row--attention' : ''}`}>
                          <div className="row__main">
                            <div className="row__title">
                              <strong>{a.title}</strong>
                              <Badge tone={a.status === 'pending' ? 'warning' : a.status === 'approved' ? 'success' : 'neutral'}>{a.status}</Badge>
                            </div>
                            <div className="row__meta">{a.description}</div>
                          </div>
                          {a.status === 'pending' && (
                            <div className="row__actions">
                              <button type="button" className="btn btn--sm" onClick={() => void rejectAction(a.id).then(() => refreshDetail(selected.spec.id))}>
                                {t('goals.reject')}
                              </button>
                              <button
                                type="button"
                                className="btn btn--primary btn--sm"
                                onClick={() => void approveAction(a.id).then(() => refreshDetail(selected.spec.id))}
                              >
                                <IconCheck size={12} />
                                {t('goals.approve')}
                              </button>
                            </div>
                          )}
                        </li>
                      ))}
                    </ul>
                  ))}

                {tab === 'policy' && (
                  <dl className="dl">
                    <dt>{t('goals.maxActions')}</dt>
                    <dd>
                      {selected.run?.actions_today ?? 0} / {selected.spec.policy.max_actions_per_day}
                    </dd>
                    <dt>{t('goals.budget')}</dt>
                    <dd>${selected.spec.policy.budget_usd}</dd>
                    <dt>{t('goals.approvalCaps')}</dt>
                    <dd>
                      {selected.spec.policy.require_approval.length === 0
                        ? '—'
                        : selected.spec.policy.require_approval.map((c) => (
                            <span key={c} className="chip chip--mono">
                              {c}
                            </span>
                          ))}
                    </dd>
                    <dt>{t('goals.tools')}</dt>
                    <dd>
                      {selected.spec.tools.map((x) => (
                        <span key={x.type_id} className="chip chip--mono">
                          {x.type_id}
                        </span>
                      ))}
                    </dd>
                  </dl>
                )}

                {tab === 'scenarios' &&
                  ((selected.linked_workflows?.length ?? 0) === 0 ? (
                    <EmptyState compact title={t('goals.noScenarios')} />
                  ) : (
                    <ul className="row-list">
                      {selected.linked_workflows?.map((wf) => (
                        <li key={wf.id} className="row">
                          <div className="row__main">
                            <div className="row__title">
                              <strong>{wf.name}</strong>
                              <span className="chip chip--mono">v{wf.version}</span>
                              <Badge tone={wf.status === 'active' ? 'success' : 'neutral'}>{wf.status}</Badge>
                            </div>
                            {wf.description && <div className="row__meta">{wf.description}</div>}
                          </div>
                          {onOpenWorkflow && (
                            <div className="row__actions">
                              <button
                                type="button"
                                className="btn btn--sm"
                                onClick={() => {
                                  onOpenWorkflow(wf.id)
                                  onClose()
                                }}
                              >
                                <IconExternal size={12} />
                                {t('goals.openCanvas')}
                              </button>
                            </div>
                          )}
                        </li>
                      ))}
                    </ul>
                  ))}
              </div>
            </>
          )}
        </section>
      </div>
    </Modal>
  )
}
