import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  extensions,
  ph,
  phAi,
  type ExtensionStatus,
  type PhBrdd,
  type PhDetail,
  type PhProject,
  type PhTask,
  type PhTaskStatus,
} from '../api/projecthub'
import { createObjective } from '../api/objectives'
import { useExtensions } from '../extensions/ExtensionsContext'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import { Modal, useConfirm } from '../ui/Modal'
import { MiniMarkdown } from '../ui/MiniMarkdown'
import { Badge, Card, EmptyState, Field, Notice, SearchInput, Segmented, Tabs, type Tone } from '../ui/primitives'
import { formatDateTime, formatRelative } from '../ui/format'
import {
  IconCheck,
  IconCode,
  IconExternal,
  IconFlag,
  IconFolder,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconSettings,
  IconSparkle,
  IconSplit,
  IconTrash,
  IconListTree,
  IconTerminal,
} from '../ui/icons'

interface Props {
  open: boolean
  onClose: () => void
  /** Open the Goals panel focused on a goal. */
  onOpenGoal: (objectiveId: string) => void
}

type View = 'project' | 'tasks' | 'settings'

const PHASES = ['prepare', 'fetch', 'build', 'self_test', 'done'] as const

const STATUS_TONE: Record<PhTaskStatus, Tone> = {
  todo: 'neutral',
  in_progress: 'ai',
  done: 'success',
  cancelled: 'neutral',
}

const PRIORITY_TONE: Record<string, Tone> = {
  low: 'neutral',
  medium: 'info',
  high: 'warning',
  critical: 'error',
}

const LANG_LABELS: Record<string, string> = {
  type_script: 'TypeScript',
  java_script: 'JavaScript',
  c_sharp: 'C#',
  cpp: 'C++',
  php: 'PHP',
}

function langLabel(lang: string | null | undefined): string {
  if (!lang) return '—'
  return LANG_LABELS[lang] ?? lang.split('_').map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join('')
}

export function ProjectHubPanel({ open, onClose, onOpenGoal }: Props) {
  const { t } = usePreferences()
  const { projecthub, setProjecthub, refresh } = useExtensions()
  const status = projecthub

  // Live install progress.
  useEffect(() => {
    if (!open) return
    void refresh()
    if (!status?.install.running && status?.state !== 'starting') return
    const id = window.setInterval(() => void refresh(), 1000)
    return () => window.clearInterval(id)
  }, [open, status?.install.running, status?.state, refresh])

  const ready = Boolean(status?.installed && status.enabled)

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      flush
      icon={<IconFolder size={18} />}
      title={t('ph.title')}
      subtitle={t('ph.subtitle')}
      actions={
        status ? (
          <Badge tone={status.running ? 'success' : status.state === 'error' ? 'error' : 'neutral'} dot>
            {t(`ph.state.${status.state}` as MessageKey)}
          </Badge>
        ) : null
      }
    >
      {!status ? (
        <div className="ph-loading">
          <span className="spinner" />
        </div>
      ) : ready ? (
        <ProjectHubWorkspace status={status} onStatus={setProjecthub} onOpenGoal={onOpenGoal} />
      ) : (
        <ProjectHubSetup status={status} onStatus={setProjecthub} />
      )}
    </Modal>
  )
}

// ─── Install / enable ─────────────────────────────────────────────────────────

function ProjectHubSetup({ status, onStatus }: { status: ExtensionStatus; onStatus: (s: ExtensionStatus) => void }) {
  const { t } = usePreferences()
  const confirm = useConfirm()
  const [repo, setRepo] = useState(status.config.repo)
  const [branch, setBranch] = useState(status.config.branch)
  const [busy, setBusy] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const logRef = useRef<HTMLPreElement>(null)
  const job = status.install
  const report = status.config.self_test

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight })
  }, [job.log.length])

  async function act(key: string, fn: () => Promise<ExtensionStatus>) {
    setBusy(key)
    setMessage(null)
    try {
      onStatus(await fn())
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(null)
    }
  }

  const phaseIdx = PHASES.indexOf(job.phase as (typeof PHASES)[number])
  const found = status.detected?.servers ?? []
  const appsWithoutServer = (status.detected?.apps ?? []).filter((a) => !a.has_server)
  const db = status.detected?.database
  const source = status.config.source ?? 'build'
  const external = source !== 'build'

  return (
    <div className="ph-setup">
      <Notice text={message} onDismiss={() => setMessage(null)} />
      <div className="ph-hero">
        <span className="ph-hero__icon">
          <IconFolder size={26} />
        </span>
        <div>
          <h3>{t('ph.setup.title')}</h3>
          <p>{t('ph.setup.text')}</p>
          <ul className="ph-features">
            {(['code', 'brdd', 'tasks', 'ai', 'github', 'goals'] as const).map((f) => (
              <li key={f}>
                <IconCheck size={13} /> {t(`ph.feature.${f}` as MessageKey)}
              </li>
            ))}
          </ul>
        </div>
      </div>

      {status.state === 'missing' && (
        <p className="tone-error">{t('ph.found.missing', { path: status.config.binary ?? '—' })}</p>
      )}

      {(!status.installed || status.state === 'missing') && !job.running && found.length > 0 && (
        <Card
          title={t('ph.found.title')}
          subtitle={t('ph.found.subtitle')}
          actions={
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              disabled={busy !== null}
              onClick={() => void act('detect', extensions.detect)}
            >
              <IconRefresh size={13} />
              {t('ph.found.rescan')}
            </button>
          }
        >
          <ul className="ph-found">
            {found.map((c, i) => (
              <li key={c.binary}>
                <div className="ph-found__main">
                  <b>{t(`ph.found.source.${c.source}` as MessageKey)}</b>
                  <span className="muted">{c.version ? `v${c.version}` : t('ph.found.oldVersion')}</span>
                  <code className="ph-found__path">{c.app ?? c.binary}</code>
                </div>
                <button
                  type="button"
                  className={i === 0 ? 'btn btn--primary' : 'btn'}
                  disabled={busy !== null}
                  onClick={() => void act(`use:${c.binary}`, () => extensions.useInstalled(c.binary))}
                >
                  {busy === `use:${c.binary}` ? <span className="spinner spinner--xs" /> : <IconCheck size={14} />}
                  {t('ph.found.use')}
                </button>
              </li>
            ))}
          </ul>
          {db && (
            <p className="field__hint">
              <b>{t('ph.found.db')}:</b>{' '}
              {db.exists
                ? `${t('ph.found.dbCounts', { projects: db.projects ?? '?', tasks: db.tasks ?? '?' })} · `
                : `${t('ph.found.noDb')} `}
              <code>{db.path}</code>
            </p>
          )}
        </Card>
      )}

      {!status.installed && !job.running &&
        appsWithoutServer.map((a) => (
          <p key={a.path} className="field__hint">
            {t('ph.found.appNoServer', { version: a.version ?? '', path: a.path })}
          </p>
        ))}

      {!status.installed && !job.running && (
        <Card title={found.length > 0 ? t('ph.found.or') : t('ph.setup.source')} subtitle={t('ph.setup.sourceHint')}>
          <div className="form">
            <div className="form__row">
              <Field label={t('ph.setup.repo')}>
                <input value={repo} onChange={(e) => setRepo(e.target.value)} spellCheck={false} />
              </Field>
              <Field label={t('ph.setup.branch')}>
                <input value={branch} onChange={(e) => setBranch(e.target.value)} spellCheck={false} />
              </Field>
            </div>
            <p className="field__hint mono">{t('ph.setup.installRoot', { path: status.install_root })}</p>
            <div className="form__actions">
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy !== null || !repo.trim()}
                onClick={() => void act('install', () => extensions.install({ repo, branch }))}
              >
                <IconPlay size={14} />
                {t('ph.setup.install')}
              </button>
            </div>
          </div>
        </Card>
      )}

      {(job.running || job.log.length > 0) && (
        <Card
          title={job.running ? t('ph.setup.installing') : job.phase === 'failed' ? t('ph.setup.failed') : t('ph.setup.log')}
          actions={job.running ? <span className="spinner spinner--xs" /> : null}
        >
          <ol className="ph-phases">
            {PHASES.map((p, i) => (
              <li
                key={p}
                className={
                  job.phase === 'failed' && i === Math.max(phaseIdx, 0)
                    ? 'is-failed'
                    : i < phaseIdx || job.phase === 'done'
                      ? 'is-done'
                      : i === phaseIdx
                        ? 'is-active'
                        : ''
                }
              >
                {t(`ph.phase.${p}` as MessageKey)}
              </li>
            ))}
          </ol>
          <pre ref={logRef} className="ph-log">
            {job.log.join('\n')}
          </pre>
          {job.error && <p className="tone-error">{job.error}</p>}
        </Card>
      )}

      {status.installed && (
        <Card
          title={t('ph.setup.selfTest')}
          subtitle={[
            t('ph.setup.sourceLine', {
              source: `${t(`ph.found.source.${source}` as MessageKey)}${status.config.version ? ` v${status.config.version}` : ''}`,
              path: status.config.binary ?? '—',
            }),
            report ? t('ph.setup.selfTestSummary', { passed: report.passed, total: report.steps.length, ms: report.duration_ms }) : null,
          ]
            .filter(Boolean)
            .join(' · ')}
          actions={
            <Badge tone={status.self_test_ok ? 'success' : 'error'} dot>
              {status.self_test_ok ? t('ph.setup.passed') : t('ph.setup.notPassed')}
            </Badge>
          }
        >
          {report && (
            <ul className="ph-steps">
              {report.steps.map((s) => (
                <li key={s.name} className={s.ok ? 'is-ok' : 'is-failed'}>
                  <span>{s.ok ? '✓' : '✕'}</span>
                  <span>{s.name}</span>
                  <span className="muted">{s.ms} ms</span>
                  {s.detail && <code>{s.detail}</code>}
                </li>
              ))}
            </ul>
          )}
          <div className="form__actions">
            <button
              type="button"
              className="btn btn--primary"
              disabled={busy !== null || !status.self_test_ok}
              onClick={() => void act('enable', extensions.enable)}
            >
              {busy === 'enable' ? <span className="spinner spinner--xs" /> : <IconCheck size={14} />}
              {t('ph.setup.enable')}
            </button>
            <button
              type="button"
              className="btn"
              disabled={busy !== null}
              onClick={() => void act('test', async () => (await extensions.selfTest()).status)}
            >
              <IconRefresh size={14} />
              {t('ph.setup.rerunTest')}
            </button>
            <div className="toolbar__spacer" />
            <button
              type="button"
              className="btn btn--ghost"
              disabled={busy !== null || job.running}
              onClick={() => void act('reinstall', () => extensions.install({ repo, branch }))}
            >
              {t('ph.setup.reinstall')}
            </button>
            <button
              type="button"
              className="btn btn--ghost"
              disabled={busy !== null || job.running}
              onClick={async () => {
                const label = external ? t('ph.setup.disconnect') : t('ph.setup.uninstall')
                const ok = await confirm({
                  title: label,
                  message: external ? t('ph.setup.uninstallConfirmExternal') : t('ph.setup.uninstallConfirm'),
                  confirmLabel: label,
                  danger: !external,
                })
                if (ok) await act('uninstall', extensions.uninstall)
              }}
            >
              <IconTrash size={13} />
              {external ? t('ph.setup.disconnect') : t('ph.setup.uninstall')}
            </button>
          </div>
        </Card>
      )}
    </div>
  )
}

// ─── Workspace (enabled) ──────────────────────────────────────────────────────

function ProjectHubWorkspace({
  status,
  onStatus,
  onOpenGoal,
}: {
  status: ExtensionStatus
  onStatus: (s: ExtensionStatus) => void
  onOpenGoal: (id: string) => void
}) {
  const { t } = usePreferences()
  const [view, setView] = useState<View>('project')
  const [query, setQuery] = useState('')
  const [projects, setProjects] = useState<PhProject[]>([])
  const [total, setTotal] = useState(0)
  const [selected, setSelected] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [rootPath, setRootPath] = useState('')
  const [showRoot, setShowRoot] = useState(false)
  const [scanning, setScanning] = useState(false)

  async function addRoot() {
    const path = rootPath.trim()
    if (!path) return
    setScanning(true)
    setMessage(null)
    try {
      const res = await ph.scan(path)
      setRootPath('')
      setShowRoot(false)
      setMessage(t('ph.rootAdded', { count: res.projects.length }))
      await load(query)
    } catch (err) {
      setMessage(String(err))
    } finally {
      setScanning(false)
    }
  }

  const rootForm = (
    <form
      className="ph-root"
      onSubmit={(e) => {
        e.preventDefault()
        void addRoot()
      }}
    >
      <input
        value={rootPath}
        onChange={(e) => setRootPath(e.target.value)}
        placeholder={t('ph.rootPlaceholder')}
        spellCheck={false}
        autoFocus
      />
      <button type="submit" className="btn btn--primary btn--sm" disabled={scanning || !rootPath.trim()}>
        {scanning ? <span className="spinner spinner--xs" /> : <IconPlus size={13} />}
        {t('ph.addRoot')}
      </button>
    </form>
  )

  const load = useCallback(async (q: string) => {
    try {
      const res = await ph.projects({ q, limit: 300 })
      setProjects(res.items)
      setTotal(res.total)
      setSelected((cur) => cur ?? res.items[0]?.id ?? null)
    } catch (err) {
      setMessage(String(err))
    }
  }, [])

  useEffect(() => {
    if (!status.running) return
    const id = window.setTimeout(() => void load(query), 250)
    return () => window.clearTimeout(id)
  }, [query, load, status.running])

  return (
    <div className="mission ph">
      <aside className="mission__sidebar">
        <Segmented
          value={view}
          onChange={setView}
          options={[
            { value: 'project', label: t('ph.view.projects'), icon: <IconFolder size={13} /> },
            { value: 'tasks', label: t('ph.view.tasks'), icon: <IconListTree size={13} /> },
            { value: 'settings', label: '', icon: <IconSettings size={13} /> },
          ]}
        />
        <SearchInput value={query} onChange={setQuery} placeholder={t('ph.search')} />
        <span className="eyebrow ph-eyebrow">
          {t('ph.projects')} <span className="section-title__count">{total}</span>
          <button
            type="button"
            className="icon-btn icon-btn--sm"
            onClick={() => setShowRoot((v) => !v)}
            data-tip={t('ph.addRoot')}
            aria-label={t('ph.addRoot')}
          >
            <IconPlus size={12} />
          </button>
        </span>
        {showRoot && rootForm}
        {!status.running && <p className="mission__empty">{t('ph.starting')}</p>}
        <ul className="mission__list ph-projects">
          {projects.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                className={`mission-item ph-project${selected === p.id && view === 'project' ? ' is-active' : ''}`}
                onClick={() => {
                  setSelected(p.id)
                  setView('project')
                }}
              >
                <span className="mission-item__top">
                  <strong>{p.name}</strong>
                  {p.git_dirty && <span className="ph-dirty" title={t('ph.dirty')} />}
                </span>
                <span className="mission-item__meta">
                  {langLabel(p.language)} · {p.root_path.split('/').slice(-2).join('/')}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <section className="mission__main">
        <Notice text={message} onDismiss={() => setMessage(null)} />
        {view === 'project' &&
          (selected ? (
            <ProjectView key={selected} id={selected} status={status} onOpenGoal={onOpenGoal} onMessage={setMessage} />
          ) : (
            <EmptyState icon={<IconFolder size={24} />} title={t('ph.noProjects')} hint={t('ph.noProjectsHint')} action={rootForm} />
          ))}
        {view === 'tasks' && <AllTasks projects={projects} onMessage={setMessage} />}
        {view === 'settings' && <ExtensionSettings status={status} onStatus={onStatus} onMessage={setMessage} />}
      </section>
    </div>
  )
}

function ProjectView({
  id,
  status,
  onOpenGoal,
  onMessage,
}: {
  id: string
  status: ExtensionStatus
  onOpenGoal: (id: string) => void
  onMessage: (m: string | null) => void
}) {
  const { t, locale } = usePreferences()
  const [detail, setDetail] = useState<PhDetail | null>(null)
  const [brdd, setBrdd] = useState<PhBrdd | null>(null)
  const [tab, setTab] = useState<'brdd' | 'tasks' | 'git' | 'commands'>('brdd')
  const [brddTab, setBrddTab] = useState<'summary' | 'changes' | 'notes'>('summary')
  const [busy, setBusy] = useState<string | null>(null)
  const [wish, setWish] = useState('')
  const [drafts, setDrafts] = useState<{ title: string; description?: string; priority?: string; pick: boolean }[]>([])
  const [newTask, setNewTask] = useState('')
  const [goalTitle, setGoalTitle] = useState('')
  const [output, setOutput] = useState<string | null>(null)

  const reload = useCallback(async () => {
    const [d, b] = await Promise.all([ph.detail(id), ph.brdd(id)])
    setDetail(d)
    setBrdd(b)
  }, [id])

  useEffect(() => {
    void reload().catch((err) => onMessage(String(err)))
  }, [reload, onMessage])

  // Statuses change in the Project Hub app too — keep the task tab current.
  useEffect(() => {
    if (tab !== 'tasks') return
    const timer = window.setInterval(() => void reload().catch(() => {}), 15_000)
    return () => window.clearInterval(timer)
  }, [tab, reload])

  async function act(key: string, fn: () => Promise<string | void>) {
    setBusy(key)
    onMessage(null)
    try {
      const msg = await fn()
      if (msg) onMessage(msg)
    } catch (err) {
      onMessage(String(err))
    } finally {
      setBusy(null)
    }
  }

  if (!detail) {
    return <div className="skeleton skeleton--row" />
  }
  const p = detail.project
  const tasks = detail.tasks
  const open = tasks.filter((x) => x.status === 'todo' || x.status === 'in_progress').length

  return (
    <div className="ph-project-view">
      <div className="obj-hero">
        <div className="obj-hero__top">
          <h3 className="ph-title">{p.name}</h3>
          {brdd?.analysis?.version && <Badge tone="info">{brdd.analysis.version}</Badge>}
          {detail.git && (
            <Badge tone={detail.git.status.modified + detail.git.status.untracked > 0 ? 'warning' : 'success'} dot>
              {detail.git.status.branch}
            </Badge>
          )}
          {detail.github_repo && (
            <a className="chip chip--btn" href={`https://github.com/${detail.github_repo}`} target="_blank" rel="noreferrer">
              <IconCode size={12} /> {detail.github_repo}
            </a>
          )}
        </div>
        <p className="mono ph-path">{p.root_path}</p>
        <div className="kv-chips">
          <span>
            {t('ph.lang')} <b>{langLabel(p.language)}</b>
          </span>
          {(brdd?.analysis?.stack ?? []).map((s) => (
            <span key={s}>{s}</span>
          ))}
          {brdd?.analysis && (
            <span>
              {t('ph.size')} <b>{brdd.analysis.structure.files}</b> / <b>{brdd.analysis.structure.loc}</b> LOC
            </span>
          )}
          <span>
            {t('ph.openTasks')} <b>{open}</b>
          </span>
        </div>
        <div className="mission-hero__actions">
          <button
            type="button"
            className="btn"
            disabled={busy !== null}
            onClick={() =>
              void act('brdd', async () => {
                await ph.brddRefresh(id)
                await reload()
                return t('ph.brddRefreshed')
              })
            }
          >
            {busy === 'brdd' ? <span className="spinner spinner--xs" /> : <IconRefresh size={14} />}
            {brdd?.exists ? t('ph.brddRefresh') : t('ph.brddCreate')}
          </button>
          <button
            type="button"
            className="btn obj-ai-btn"
            disabled={busy !== null}
            onClick={() =>
              void act('analyze', async () => {
                const res = await phAi.analyze(id, locale)
                setBrdd(res.bundle)
                setTab('brdd')
                setBrddTab('notes')
                return t('ph.analyzed', { model: res.model })
              })
            }
          >
            {busy === 'analyze' ? <span className="spinner spinner--xs" /> : <IconSparkle size={14} />}
            {t('ph.aiAnalyze')}
          </button>
          <span className="obj-sep" />
          <input
            className="obj-wish"
            value={goalTitle}
            onChange={(e) => setGoalTitle(e.target.value)}
            placeholder={t('ph.goalPlaceholder')}
          />
          <button
            type="button"
            className="btn"
            disabled={busy !== null || !goalTitle.trim()}
            onClick={() =>
              void act('goal', async () => {
                const o = await createObjective({
                  title: goalTitle.trim(),
                  description: t('ph.goalDescription', { name: p.name }),
                  project_hub_project_id: p.id,
                  project_hub_project_name: p.name,
                })
                setGoalTitle('')
                onOpenGoal(o.id)
              })
            }
          >
            <IconFlag size={14} />
            {t('ph.createGoal')}
          </button>
        </div>
      </div>

      <Tabs
        value={tab}
        onChange={setTab}
        items={[
          { id: 'brdd', label: '.brdd', icon: <IconFlag size={14} />, count: brdd?.versions.length },
          { id: 'tasks', label: t('ph.tab.tasks'), icon: <IconListTree size={14} />, count: tasks.length },
          { id: 'git', label: 'Git', icon: <IconSplit size={14} /> },
          { id: 'commands', label: t('ph.tab.commands'), icon: <IconTerminal size={14} />, count: detail.commands.length },
        ]}
      />

      <div className="mission__tab">
        {tab === 'brdd' &&
          (!brdd?.exists ? (
            <EmptyState compact icon={<IconFlag size={20} />} title={t('ph.brddEmpty')} hint={t('ph.brddEmptyHint')} />
          ) : (
            <div className="ph-brdd">
              <Segmented
                value={brddTab}
                onChange={setBrddTab}
                options={[
                  { value: 'summary', label: t('ph.brdd.summary') },
                  { value: 'changes', label: t('ph.brdd.changes') },
                  { value: 'notes', label: t('ph.brdd.notes') },
                ]}
              />
              <div className="obj-result">
                {brddTab === 'summary' && <MiniMarkdown text={brdd.summary_md ?? '—'} />}
                {brddTab === 'changes' && <MiniMarkdown text={brdd.changelog_md ?? '—'} />}
                {brddTab === 'notes' &&
                  (brdd.notes_md ? <MiniMarkdown text={brdd.notes_md} /> : <p className="muted">{t('ph.brdd.notesEmpty')}</p>)}
              </div>
              <p className="field__hint mono">{brdd.dir}</p>
            </div>
          ))}

        {tab === 'tasks' && (
          <div className="ph-tasks">
            <Card title={t('ph.suggest.title')} subtitle={t('ph.suggest.hint')}>
              <div className="ph-suggest">
                <input value={wish} onChange={(e) => setWish(e.target.value)} placeholder={t('ph.suggest.placeholder')} />
                <button
                  type="button"
                  className="btn obj-ai-btn"
                  disabled={busy !== null}
                  onClick={() =>
                    void act('suggest', async () => {
                      const res = await phAi.suggestTasks(id, { instruction: wish, count: 5, apply: false, locale })
                      setDrafts(res.tasks.map((d) => ({ ...d, pick: true })))
                      return res.tasks.length ? undefined : t('ph.suggest.none')
                    })
                  }
                >
                  {busy === 'suggest' ? <span className="spinner spinner--xs" /> : <IconSparkle size={14} />}
                  {t('ph.suggest.run')}
                </button>
              </div>
              {drafts.length > 0 && (
                <>
                  <ul className="ph-drafts">
                    {drafts.map((d, i) => (
                      <li key={i}>
                        <label>
                          <input
                            type="checkbox"
                            checked={d.pick}
                            onChange={(e) => setDrafts((prev) => prev.map((x, j) => (j === i ? { ...x, pick: e.target.checked } : x)))}
                          />
                          <strong>{d.title}</strong>
                          {d.priority && <Badge tone={PRIORITY_TONE[d.priority] ?? 'neutral'}>{d.priority}</Badge>}
                        </label>
                        {d.description && <p className="muted">{d.description}</p>}
                      </li>
                    ))}
                  </ul>
                  <div className="form__actions">
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={busy !== null || !drafts.some((d) => d.pick)}
                      onClick={() =>
                        void act('create-drafts', async () => {
                          const picked = drafts.filter((d) => d.pick)
                          for (const d of picked) {
                            await ph.createTask({
                              project_id: id,
                              title: d.title,
                              description: d.description,
                              priority: d.priority,
                              external_ref: 'ai:suggest-tasks',
                            })
                          }
                          setDrafts([])
                          await reload()
                          return t('ph.suggest.created', { count: picked.length })
                        })
                      }
                    >
                      <IconPlus size={14} />
                      {t('ph.suggest.create')}
                    </button>
                    <button type="button" className="btn btn--ghost" onClick={() => setDrafts([])}>
                      {t('common.cancel')}
                    </button>
                  </div>
                </>
              )}
            </Card>

            <form
              className="ph-new-task"
              onSubmit={(e) => {
                e.preventDefault()
                const title = newTask.trim()
                if (!title) return
                void act('task', async () => {
                  await ph.createTask({ project_id: id, title })
                  setNewTask('')
                  await reload()
                })
              }}
            >
              <input value={newTask} onChange={(e) => setNewTask(e.target.value)} placeholder={t('ph.newTask')} />
              <button type="submit" className="btn" disabled={!newTask.trim() || busy !== null}>
                <IconPlus size={14} />
                {t('ph.addTask')}
              </button>
            </form>

            <TaskRows tasks={tasks} onChanged={reload} onMessage={onMessage} githubRepo={detail.github_repo} />
          </div>
        )}

        {tab === 'git' &&
          (detail.git ? (
            <div className="ph-git">
              <div className="kv-chips">
                <span>
                  {t('ph.git.branch')} <b>{detail.git.status.branch}</b>
                </span>
                <span>
                  {t('ph.git.modified')} <b>{detail.git.status.modified}</b>
                </span>
                <span>
                  {t('ph.git.untracked')} <b>{detail.git.status.untracked}</b>
                </span>
                <span>
                  ↑ <b>{detail.git.status.ahead}</b> ↓ <b>{detail.git.status.behind}</b>
                </span>
              </div>
              <ul className="obj-log">
                {detail.git.recent_commits.map((c) => (
                  <li key={c.hash} className="obj-log__entry">
                    <time>{c.hash}</time>
                    <span>{c.message}</span>
                    <span className="muted">
                      {c.author} · {formatRelative(c.date, locale)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <EmptyState compact icon={<IconSplit size={20} />} title={t('ph.git.none')} />
          ))}

        {tab === 'commands' && (
          <div className="ph-commands">
            {status.config.commands === 'off' && <p className="muted">{t('ph.commandsOff')}</p>}
            <ul className="row-list">
              {detail.commands.map((c) => (
                <li key={c.command} className="row">
                  <div className="row__main">
                    <div className="row__title">
                      <strong>{c.name}</strong>
                    </div>
                    <div className="row__meta mono">{c.command}</div>
                  </div>
                  <div className="row__actions">
                    <button
                      type="button"
                      className="btn btn--sm"
                      disabled={busy !== null || status.config.commands === 'off'}
                      onClick={() =>
                        void act(`cmd:${c.command}`, async () => {
                          const r = await ph.runCommand(id, c.command)
                          setOutput(`$ ${r.command}  (exit ${r.exit_code}, ${r.duration_ms} ms)\n\n${r.stdout}${r.stderr ? `\n${r.stderr}` : ''}`)
                        })
                      }
                    >
                      {busy === `cmd:${c.command}` ? <span className="spinner spinner--xs" /> : <IconPlay size={12} />}
                      {t('ph.run')}
                    </button>
                  </div>
                </li>
              ))}
            </ul>
            {output && <pre className="ph-log">{output}</pre>}
          </div>
        )}
      </div>
    </div>
  )
}

function TaskRows({
  tasks,
  onChanged,
  onMessage,
  githubRepo,
  projectNames,
}: {
  tasks: PhTask[]
  onChanged: () => Promise<void> | void
  onMessage: (m: string | null) => void
  githubRepo?: string | null
  projectNames?: Map<string, string>
}) {
  const { t, locale } = usePreferences()
  const confirm = useConfirm()
  const [busy, setBusy] = useState<string | null>(null)

  async function act(key: string, fn: () => Promise<unknown>) {
    setBusy(key)
    try {
      await fn()
      await onChanged()
    } catch (err) {
      onMessage(String(err))
    } finally {
      setBusy(null)
    }
  }

  if (tasks.length === 0) {
    return <EmptyState compact icon={<IconListTree size={20} />} title={t('ph.tasksEmpty')} />
  }
  return (
    <ul className="ph-task-list">
      {tasks.map((task) => (
        <li key={task.id} className={`ph-task ph-task--${task.status}`}>
          <select
            className={`ph-task__status tone-${STATUS_TONE[task.status]}`}
            value={task.status}
            disabled={busy !== null}
            onChange={(e) => void act(task.id, () => ph.updateTask(task.id, { status: e.target.value as PhTaskStatus }))}
          >
            {(['todo', 'in_progress', 'done', 'cancelled'] as const).map((s) => (
              <option key={s} value={s}>
                {t(`editor.ph.status.${s}` as MessageKey)}
              </option>
            ))}
          </select>
          <div className="ph-task__main">
            <div className="ph-task__title">
              <strong>{task.title}</strong>
              <Badge tone={PRIORITY_TONE[task.priority] ?? 'neutral'}>{task.priority}</Badge>
              {task.link?.source === 'boarddo' && (
                <Badge tone="ai" title={task.link.external_ref ?? undefined}>
                  BoardDo
                </Badge>
              )}
              {task.project_id && projectNames?.get(task.project_id) && (
                <span className="chip">{projectNames.get(task.project_id)}</span>
              )}
            </div>
            {task.description && <p className="muted ph-task__desc">{task.description}</p>}
            <span className="muted ph-task__meta">
              {task.due_at ? `${t('ph.due')} ${formatDateTime(task.due_at, locale)} · ` : ''}
              {formatRelative(task.updated_at, locale)}
            </span>
          </div>
          <div className="row__actions">
            {task.link?.github_issue_url ? (
              <a className="btn btn--sm" href={task.link.github_issue_url} target="_blank" rel="noreferrer">
                <IconExternal size={12} />#{task.link.github_issue_number}
                {task.link.github_issue_state === 'closed' ? ' ✓' : ''}
              </a>
            ) : (
              (githubRepo || projectNames) &&
              task.project_id && (
                <button
                  type="button"
                  className="btn btn--sm"
                  disabled={busy !== null}
                  title={t('ph.createIssue')}
                  onClick={() => void act(`issue:${task.id}`, () => ph.createIssue(task.id))}
                >
                  {busy === `issue:${task.id}` ? <span className="spinner spinner--xs" /> : <IconCode size={12} />}
                  {t('ph.issue')}
                </button>
              )
            )}
            <button
              type="button"
              className="icon-btn icon-btn--sm icon-btn--danger"
              disabled={busy !== null}
              aria-label={t('ph.deleteTask')}
              onClick={async () => {
                const ok = await confirm({
                  title: t('ph.deleteTask'),
                  message: task.title,
                  confirmLabel: t('ph.deleteTask'),
                  danger: true,
                })
                if (ok) await act(`del:${task.id}`, () => ph.deleteTask(task.id))
              }}
            >
              <IconTrash size={12} />
            </button>
          </div>
        </li>
      ))}
    </ul>
  )
}

function AllTasks({ projects, onMessage }: { projects: PhProject[]; onMessage: (m: string | null) => void }) {
  const { t } = usePreferences()
  const [tasks, setTasks] = useState<PhTask[]>([])
  const [status, setStatus] = useState<'open' | 'all' | 'done'>('open')
  const [source, setSource] = useState<'' | 'boarddo'>('')
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    const filter = status === 'open' ? 'todo,in_progress' : status === 'done' ? 'done' : undefined
    setTasks(await ph.tasks({ status: filter, source: source || undefined }))
  }, [status, source])

  useEffect(() => {
    void load().catch((err) => onMessage(String(err)))
    const id = window.setInterval(() => void load().catch(() => {}), 15_000)
    return () => window.clearInterval(id)
  }, [load, onMessage])

  const names = useMemo(() => new Map(projects.map((p) => [p.id, p.name])), [projects])

  return (
    <div className="ph-all-tasks">
      <div className="mission-hero__actions">
        <Segmented
          value={status}
          onChange={setStatus}
          options={[
            { value: 'open', label: t('ph.filter.open') },
            { value: 'done', label: t('ph.filter.done') },
            { value: 'all', label: t('ph.filter.all') },
          ]}
        />
        <Segmented
          value={source}
          onChange={setSource}
          options={[
            { value: '', label: t('ph.filter.anySource') },
            { value: 'boarddo', label: 'BoardDo' },
          ]}
        />
        <div className="toolbar__spacer" />
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              const r = await ph.syncIssues()
              onMessage(t('ph.issuesSynced', { checked: r.checked, done: r.tasks_completed, closed: r.issues_closed }))
              await load()
            } catch (err) {
              onMessage(String(err))
            } finally {
              setBusy(false)
            }
          }}
        >
          {busy ? <span className="spinner spinner--xs" /> : <IconRefresh size={14} />}
          {t('ph.syncIssues')}
        </button>
      </div>
      <TaskRows tasks={tasks} onChanged={load} onMessage={onMessage} projectNames={names} />
    </div>
  )
}

function ExtensionSettings({
  status,
  onStatus,
  onMessage,
}: {
  status: ExtensionStatus
  onStatus: (s: ExtensionStatus) => void
  onMessage: (m: string | null) => void
}) {
  const { t, locale } = usePreferences()
  const confirm = useConfirm()
  const [busy, setBusy] = useState<string | null>(null)
  const [dbPath, setDbPath] = useState(status.config.db_path ?? '')

  async function act(key: string, fn: () => Promise<ExtensionStatus>, done?: string) {
    setBusy(key)
    onMessage(null)
    try {
      onStatus(await fn())
      if (done) onMessage(done)
    } catch (err) {
      onMessage(String(err))
    } finally {
      setBusy(null)
    }
  }

  const report = status.config.self_test
  return (
    <div className="obj-setup">
      <Card
        title={t('ph.settings.runtime')}
        actions={
          <Badge tone={status.running ? 'success' : 'warning'} dot>
            {t(`ph.state.${status.state}` as MessageKey)}
          </Badge>
        }
      >
        <div className="kv-chips">
          <span>
            {t('ph.settings.version')} <b>{status.config.version ?? '—'}</b>
          </span>
          {status.url && (
            <span>
              URL <b className="mono">{status.url}</b>
            </span>
          )}
          {status.pid && (
            <span>
              PID <b>{status.pid}</b>
            </span>
          )}
          {status.started_at && (
            <span>
              {t('ph.settings.started')} <b>{formatRelative(status.started_at, locale)}</b>
            </span>
          )}
          {report && (
            <span>
              {t('ph.setup.selfTest')} <b>{report.passed}/{report.steps.length}</b>
            </span>
          )}
        </div>
        <p className="field__hint mono">{status.config.binary}</p>
        <div className="form__actions">
          <button type="button" className="btn" disabled={busy !== null} onClick={() => void act('restart', extensions.restart)}>
            <IconRefresh size={14} />
            {t('ph.settings.restart')}
          </button>
          <button
            type="button"
            className="btn"
            disabled={busy !== null}
            onClick={() => void act('test', async () => (await extensions.selfTest()).status, t('ph.settings.testDone'))}
          >
            <IconCheck size={14} />
            {t('ph.setup.rerunTest')}
          </button>
          <button
            type="button"
            className="btn"
            disabled={busy !== null}
            onClick={async () => {
              setBusy('brdd-all')
              try {
                const r = await ph.brddRefreshAll()
                onMessage(t('ph.settings.brddAllDone', { ok: r.refreshed, failed: r.failed }))
              } catch (err) {
                onMessage(String(err))
              } finally {
                setBusy(null)
              }
            }}
          >
            {busy === 'brdd-all' ? <span className="spinner spinner--xs" /> : <IconFlag size={14} />}
            {t('ph.settings.brddAll')}
          </button>
          <div className="toolbar__spacer" />
          <button type="button" className="btn btn--ghost" disabled={busy !== null} onClick={() => void act('disable', extensions.disable)}>
            {t('ph.settings.disable')}
          </button>
          <button
            type="button"
            className="btn btn--ghost"
            disabled={busy !== null}
            onClick={async () => {
              const ok = await confirm({
                title: t('ph.setup.uninstall'),
                message: t('ph.setup.uninstallConfirm'),
                confirmLabel: t('ph.setup.uninstall'),
                danger: true,
              })
              if (ok) await act('uninstall', extensions.uninstall)
            }}
          >
            <IconTrash size={13} />
            {t('ph.setup.uninstall')}
          </button>
        </div>
      </Card>

      <Card title={t('ph.settings.commands')} subtitle={t('ph.settings.commandsHint')}>
        <Segmented
          value={status.config.commands}
          onChange={(v) => void act('commands', () => extensions.config({ commands: v }))}
          options={[
            { value: 'off', label: t('ph.settings.cmdOff') },
            { value: 'detected', label: t('ph.settings.cmdDetected') },
            { value: 'any', label: t('ph.settings.cmdAny') },
          ]}
        />
      </Card>

      <Card title={t('ph.settings.database')} subtitle={t('ph.settings.databaseHint')}>
        <div className="form">
          <Field label={t('ph.settings.dbPath')}>
            <input value={dbPath} onChange={(e) => setDbPath(e.target.value)} placeholder={t('ph.settings.dbShared')} spellCheck={false} />
          </Field>
          <div className="form__actions">
            <button
              type="button"
              className="btn btn--primary btn--sm"
              disabled={busy !== null || dbPath === (status.config.db_path ?? '')}
              onClick={() => void act('db', () => extensions.config({ db_path: dbPath }))}
            >
              {t('app.save')}
            </button>
          </div>
        </div>
      </Card>

      <Card title={t('ph.settings.source')} subtitle={`${status.config.repo} · ${status.config.branch}`}>
        <div className="form__actions">
          <button
            type="button"
            className="btn"
            disabled={busy !== null || status.install.running}
            onClick={() => void act('update', () => extensions.install({}), t('ph.settings.updating'))}
          >
            <IconRefresh size={14} />
            {t('ph.settings.update')}
          </button>
          {status.install.running && <span className="spinner spinner--xs" />}
        </div>
        {status.install.log.length > 0 && <pre className="ph-log">{status.install.log.slice(-40).join('\n')}</pre>}
      </Card>
    </div>
  )
}
