import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import {
  assistObjective,
  createObjective,
  deleteObjective,
  ensureItemScenario,
  listObjectives,
  objectivesWsUrl,
  runObjective,
  stopObjective,
  updateObjective,
  updateObjectiveItem,
} from '../api/objectives'
import { getConnections, isClaudeConnection, isLlmConnection } from '../api/connections'
import { getWorkflows } from '../api/workflows'
import { ph, type PhProject } from '../api/projecthub'
import { useExtensions } from '../extensions/ExtensionsContext'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import { Modal, useConfirm } from '../ui/Modal'
import { MiniMarkdown } from '../ui/MiniMarkdown'
import { Badge, Card, EmptyState, Field, Notice, Progress, Segmented, Tabs, type Tone } from '../ui/primitives'
import { formatDateTime, formatRelative, formatTime } from '../ui/format'
import {
  IconActivity,
  IconAlert,
  IconCheck,
  IconChevron,
  IconCopy,
  IconEdit,
  IconExternal,
  IconFlag,
  IconFolder,
  IconForward,
  IconGlobe,
  IconListTree,
  IconPause,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconSave,
  IconSettings,
  IconSparkle,
  IconSplit,
  IconTarget,
  IconTrash,
  IconUser,
} from '../ui/icons'
import type {
  Connection,
  Objective,
  ObjectiveInput,
  ObjectiveItem,
  ObjectiveItemKind,
  ObjectiveItemStatus,
  ObjectiveStatus,
  WorkflowSummary,
} from '../types'

interface Props {
  open: boolean
  onClose: () => void
  onOpenWorkflow: (id: string) => void
  onOpenAiSettings: () => void
  /** Select this goal when the panel opens (e.g. created from ProjectHub). */
  focusId?: string | null
  onOpenProjectHub?: () => void
}

type Tab = 'items' | 'setup' | 'log' | 'summary'

const MAX_DEPTH = 3

const STATUS_TONE: Record<ObjectiveStatus, Tone> = {
  draft: 'neutral',
  ready: 'info',
  running: 'ai',
  paused: 'warning',
  done: 'success',
  failed: 'error',
}

const ITEM_TONE: Record<ObjectiveItemStatus, Tone> = {
  pending: 'neutral',
  running: 'ai',
  done: 'success',
  failed: 'error',
  skipped: 'neutral',
}

const KINDS: ObjectiveItemKind[] = ['ai', 'research', 'workflow', 'manual', 'task']

const KIND_ICON: Record<ObjectiveItemKind, ReactNode> = {
  ai: <IconSparkle size={12} />,
  research: <IconGlobe size={12} />,
  workflow: <IconListTree size={12} />,
  manual: <IconUser size={12} />,
  task: <IconFolder size={12} />,
}

// ─── Tree helpers (mirror crates/shared/src/v2/objective.rs) ────────────────

function childrenOf(o: Objective, parent: string | null): ObjectiveItem[] {
  return o.items.filter((i) => (i.parent_id ?? null) === parent)
}

function isLeaf(o: Objective, id: string): boolean {
  return !o.items.some((i) => i.parent_id === id)
}

function orderedIds(o: Objective): string[] {
  const out: string[] = []
  const walk = (parent: string | null, depth: number) => {
    if (depth > MAX_DEPTH + 2) return
    for (const item of childrenOf(o, parent)) {
      if (out.includes(item.id)) continue
      out.push(item.id)
      walk(item.id, depth + 1)
    }
  }
  walk(null, 0)
  for (const i of o.items) if (!out.includes(i.id)) out.push(i.id)
  return out
}

function itemPath(o: Objective, id: string): string {
  const parts: number[] = []
  let cur = o.items.find((i) => i.id === id)
  let guard = 0
  while (cur && guard <= o.items.length) {
    const siblings = childrenOf(o, cur.parent_id ?? null)
    parts.unshift(siblings.findIndex((s) => s.id === cur!.id) + 1)
    cur = cur.parent_id ? o.items.find((i) => i.id === cur!.parent_id) : undefined
    guard += 1
  }
  return parts.join('.')
}

function descendants(o: Objective, id: string): string[] {
  const out: string[] = []
  const stack = [id]
  while (stack.length) {
    const cur = stack.pop()!
    for (const c of childrenOf(o, cur)) {
      out.push(c.id)
      stack.push(c.id)
    }
  }
  return out
}

function ancestors(o: Objective, id: string): string[] {
  const out: string[] = []
  let cur = o.items.find((i) => i.id === id)?.parent_id
  while (cur && !out.includes(cur)) {
    out.push(cur)
    cur = o.items.find((i) => i.id === cur)?.parent_id
  }
  return out
}

/** Items that (transitively) wait for `id` or anything inside it — picking them as deps would cycle. */
function dependents(o: Objective, id: string): string[] {
  const targets = new Set([id, ...descendants(o, id)])
  let grew = true
  while (grew) {
    grew = false
    for (const item of o.items) {
      if (targets.has(item.id)) continue
      const waits = item.depends_on.some((d) => targets.has(d))
      const parentIn = item.parent_id ? targets.has(item.parent_id) : false
      if (waits || parentIn) {
        targets.add(item.id)
        for (const d of descendants(o, item.id)) targets.add(d)
        grew = true
      }
    }
  }
  return [...targets]
}

function nextItemId(o: Objective, parent: string | null): string {
  const prefix = parent ? `${parent}.` : 'i'
  let n = 1
  while (o.items.some((i) => i.id === `${prefix}${n}`)) n += 1
  return `${prefix}${n}`
}

function toInput(o: Objective): ObjectiveInput {
  return {
    title: o.title,
    description: o.description,
    characteristics: o.characteristics.filter((c) => c.key.trim()),
    success_criteria: o.success_criteria.filter((c) => c.trim()),
    items: o.items,
    connection_id: o.connection_id ?? null,
    model: o.model ?? null,
    deadline: o.deadline ?? null,
    auto_refine: o.auto_refine ?? 0,
    project_hub_project_id: o.project_hub_project_id ?? null,
    project_hub_project_name: o.project_hub_project_name ?? null,
  }
}

const EXEC_FIELDS = [
  'status',
  'result',
  'error',
  'execution_id',
  'workflow_id',
  'scenario_hash',
  'external_task_id',
  'external_status',
  'external_url',
  'attempts',
  'started_at',
  'finished_at',
] as const

/** Keep local edits, take live execution state from the server. */
function mergeServer(local: Objective, server: Objective): Objective {
  const items = local.items.map((item) => {
    const s = server.items.find((x) => x.id === item.id)
    if (!s) return item
    const next = { ...item }
    for (const f of EXEC_FIELDS) (next as Record<string, unknown>)[f] = s[f]
    return next
  })
  return {
    ...local,
    items,
    status: server.status,
    progress: server.progress,
    log: server.log,
    summary: server.summary,
    achieved: server.achieved,
    refine_rounds: server.refine_rounds,
    started_at: server.started_at,
    finished_at: server.finished_at,
    updated_at: server.updated_at,
  }
}

function newItem(id: string, parent: string | null): ObjectiveItem {
  return {
    id,
    parent_id: parent,
    title: '',
    description: '',
    kind: 'ai',
    status: 'pending',
    depends_on: [],
    attempts: 0,
  }
}

function connectionModels(c: Connection | undefined): string[] {
  const raw = (c?.config as Record<string, unknown> | undefined)?.models
  if (!Array.isArray(raw)) return []
  return raw
    .map((m) => (typeof m === 'string' ? m : String((m as Record<string, unknown>)?.id ?? '')))
    .filter(Boolean)
}

function elapsed(from?: string | null, to?: string | null): string {
  if (!from) return ''
  const ms = (to ? new Date(to).getTime() : Date.now()) - new Date(from).getTime()
  if (!Number.isFinite(ms) || ms < 0) return ''
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  return m < 60 ? `${m}m ${s % 60}s` : `${Math.floor(m / 60)}h ${m % 60}m`
}

// ─── Panel ───────────────────────────────────────────────────────────────────

export function ObjectivesPanel({ open, onClose, onOpenWorkflow, onOpenAiSettings, focusId, onOpenProjectHub }: Props) {
  const { t, locale } = usePreferences()
  const { projecthubReady } = useExtensions()
  const [phProjects, setPhProjects] = useState<PhProject[]>([])
  const confirm = useConfirm()
  const [list, setList] = useState<Objective[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draft, setDraft] = useState<Objective | null>(null)
  const [dirty, setDirty] = useState(false)
  const [tab, setTab] = useState<Tab>('items')
  const [busy, setBusy] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [connections, setConnections] = useState<Connection[]>([])
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([])
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [wish, setWish] = useState('')
  const [newText, setNewText] = useState('')
  const [live, setLive] = useState(false)
  const [, setTick] = useState(0)

  // Mirrors for socket / async callbacks; writers also update them eagerly.
  const selectedRef = useRef<string | null>(null)
  const dirtyRef = useRef(false)
  useEffect(() => {
    selectedRef.current = selectedId
  }, [selectedId])
  useEffect(() => {
    dirtyRef.current = dirty
  }, [dirty])

  const applyServer = useCallback((server: Objective) => {
    setList((prev) => {
      const idx = prev.findIndex((o) => o.id === server.id)
      if (idx < 0) return [server, ...prev]
      const next = [...prev]
      next[idx] = server
      return next
    })
    if (server.id !== selectedRef.current) return
    setDraft((prev) => (prev && dirtyRef.current && prev.id === server.id ? mergeServer(prev, server) : server))
  }, [])

  const refreshAll = useCallback(async () => {
    try {
      const items = await listObjectives()
      setList(items)
      const sel = items.find((o) => o.id === selectedRef.current)
      if (sel) applyServer(sel)
    } catch (err) {
      setMessage(String(err))
    }
  }, [applyServer])

  // Initial load + reference data.
  useEffect(() => {
    if (!open) return
    setMessage(null)
    void (async () => {
      try {
        const [items, conns, wfs] = await Promise.all([
          listObjectives(),
          getConnections().catch(() => [] as Connection[]),
          getWorkflows().catch(() => [] as WorkflowSummary[]),
        ])
        setList(items)
        setConnections(conns.filter((c) => c.enabled && isLlmConnection(c)))
        setWorkflows(wfs)
        if (!selectedRef.current && items[0]) {
          selectedRef.current = items[0].id
          setSelectedId(items[0].id)
          setDraft(items[0])
        }
      } catch (err) {
        setMessage(String(err))
      }
    })()
  }, [open])

  useEffect(() => {
    if (!open || !projecthubReady) return
    ph.projects({ limit: 1000 })
      .then((r) => setPhProjects(r.items))
      .catch(() => setPhProjects([]))
  }, [open, projecthubReady])

  // Focus a goal created elsewhere (ProjectHub → "Goal for this project").
  useEffect(() => {
    if (!open || !focusId) return
    void listObjectives().then((items) => {
      setList(items)
      const target = items.find((o) => o.id === focusId)
      if (target) {
        selectedRef.current = target.id
        dirtyRef.current = false
        setSelectedId(target.id)
        setDraft(target)
        setDirty(false)
        setTab('items')
      }
    })
  }, [open, focusId])

  // Live updates over the goals socket (debounced), reconnecting.
  const refreshTimer = useRef<number | undefined>(undefined)
  useEffect(() => {
    if (!open) return
    let ws: WebSocket | null = null
    let closed = false
    let retry: number | undefined
    const connect = () => {
      ws = new WebSocket(objectivesWsUrl())
      ws.onopen = () => setLive(true)
      ws.onclose = () => {
        setLive(false)
        if (!closed) retry = window.setTimeout(connect, 2500)
      }
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(String(ev.data)) as { type?: string }
          if (msg.type !== 'objective_updated') return
        } catch {
          return
        }
        window.clearTimeout(refreshTimer.current)
        refreshTimer.current = window.setTimeout(() => void refreshAll(), 250)
      }
    }
    connect()
    return () => {
      closed = true
      window.clearTimeout(retry)
      window.clearTimeout(refreshTimer.current)
      ws?.close()
    }
  }, [open, refreshAll])

  const anyRunning = list.some((o) => o.status === 'running')

  // Fallback polling when the socket is down + a ticking clock for elapsed times.
  useEffect(() => {
    if (!open || !anyRunning) return
    const id = window.setInterval(() => {
      setTick((n) => n + 1)
      if (!live) void refreshAll()
    }, live ? 1000 : 2500)
    return () => window.clearInterval(id)
  }, [open, anyRunning, live, refreshAll])

  function select(id: string) {
    if (id === selectedId) return
    const o = list.find((x) => x.id === id) ?? null
    selectedRef.current = id
    dirtyRef.current = false
    setSelectedId(id)
    setDraft(o)
    setDirty(false)
    setExpanded(new Set())
    setTab('items')
    setWish('')
  }

  function edit(fn: (o: Objective) => Objective) {
    setDraft((prev) => (prev ? fn(prev) : prev))
    dirtyRef.current = true
    setDirty(true)
  }

  function editItem(id: string, patch: Partial<ObjectiveItem>) {
    edit((o) => ({ ...o, items: o.items.map((i) => (i.id === id ? { ...i, ...patch } : i)) }))
  }

  async function act(key: string, fn: () => Promise<void>) {
    setBusy(key)
    setMessage(null)
    try {
      await fn()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(null)
    }
  }

  /** Persist local edits before anything that runs on the server. */
  async function flush(): Promise<Objective | null> {
    if (!draft) return null
    if (!dirty) return draft
    const saved = await updateObjective(draft.id, toInput(draft))
    setDirty(false)
    dirtyRef.current = false
    applyServer(saved)
    setDraft(saved)
    return saved
  }

  async function save() {
    await act('save', async () => {
      await flush()
      setMessage(t('obj.saved'))
    })
  }

  function discard() {
    const server = list.find((o) => o.id === selectedId) ?? null
    dirtyRef.current = false
    setDraft(server)
    setDirty(false)
  }

  async function create(withAi: boolean) {
    const text = newText.trim()
    if (!text) return
    const [first, ...rest] = text.split('\n')
    await act(withAi ? 'create-ai' : 'create', async () => {
      const created = await createObjective({
        title: first.trim().slice(0, 160),
        description: rest.join('\n').trim(),
      })
      applyServer(created)
      setSelectedId(created.id)
      selectedRef.current = created.id
      setDraft(created)
      setDirty(false)
      setNewText('')
      setTab(withAi ? 'items' : 'setup')
      if (withAi) {
        const res = await assistObjective(created.id, { mode: 'fill', apply: true, locale })
        applyServer(res.objective)
        setDraft(res.objective)
        setMessage(res.draft.message || t('obj.aiDone', { model: res.model }))
      }
    })
  }

  async function assist(mode: 'fill' | 'decompose', itemId?: string) {
    await act(itemId ? `split:${itemId}` : mode, async () => {
      const current = await flush()
      if (!current) return
      const res = await assistObjective(current.id, {
        mode,
        item_id: itemId ?? null,
        instruction: wish.trim() || null,
        apply: true,
        locale,
      })
      applyServer(res.objective)
      setDraft(res.objective)
      setDirty(false)
      setWish('')
      if (itemId) setExpanded((prev) => new Set(prev).add(itemId))
      setMessage(`${res.draft.message || ''} ${t('obj.aiDone', { model: res.model })}`.trim())
    })
  }

  /** AI appends gap-closing items after a "not achieved" evaluation, then the run continues. */
  async function refine() {
    await act('refine', async () => {
      const current = await flush()
      if (!current) return
      const res = await assistObjective(current.id, {
        mode: 'refine',
        instruction: wish.trim() || null,
        apply: true,
        locale,
      })
      applyServer(res.objective)
      setDraft(res.objective)
      setWish('')
      setTab('items')
      applyServer(await runObjective(current.id))
    })
  }

  async function run(opts: { retry_failed?: boolean; reset?: boolean; item_id?: string } = {}) {
    if (opts.reset) {
      const ok = await confirm({
        title: t('obj.rerun'),
        message: t('obj.confirmRerun'),
        confirmLabel: t('obj.rerun'),
      })
      if (!ok) return
    }
    await act(opts.item_id ? `run:${opts.item_id}` : 'run', async () => {
      const current = await flush()
      if (!current) return
      applyServer(await runObjective(current.id, opts))
    })
  }

  async function stop() {
    if (!draft) return
    await act('stop', async () => applyServer(await stopObjective(draft.id)))
  }

  async function setItemStatus(itemId: string, status: ObjectiveItemStatus) {
    await act(`status:${itemId}`, async () => {
      const current = await flush()
      if (!current) return
      applyServer(await updateObjectiveItem(current.id, itemId, { status }))
    })
  }

  async function linkWorkflow(itemId: string, workflowId: string) {
    await act(`link:${itemId}`, async () => {
      const current = await flush()
      if (!current) return
      const next = workflowId
        ? await updateObjectiveItem(current.id, itemId, { workflow_id: workflowId })
        : await updateObjectiveItem(current.id, itemId, { unlink_workflow: true })
      applyServer(next)
      setDraft(next)
    })
  }

  async function openScenario(item: ObjectiveItem) {
    await act(`open:${item.id}`, async () => {
      let wid = item.workflow_id
      if (!wid) {
        const current = await flush()
        if (!current) return
        const next = await ensureItemScenario(current.id, item.id)
        applyServer(next)
        wid = next.items.find((i) => i.id === item.id)?.workflow_id
      }
      if (wid) onOpenWorkflow(wid)
    })
  }

  async function remove() {
    if (!draft) return
    const ok = await confirm({
      title: t('obj.delete'),
      message: t('obj.confirmDelete', { name: draft.title }),
      confirmLabel: t('obj.delete'),
      danger: true,
    })
    if (!ok) return
    await act('delete', async () => {
      await deleteObjective(draft.id)
      const rest = list.filter((o) => o.id !== draft.id)
      setList(rest)
      selectedRef.current = rest[0]?.id ?? null
      setSelectedId(rest[0]?.id ?? null)
      setDraft(rest[0] ?? null)
      setDirty(false)
    })
  }

  function addItem(parent: string | null) {
    if (!draft) return
    const id = nextItemId(draft, parent)
    edit((o) => {
      const items = [...o.items]
      // Insert after the parent's last descendant to keep tree order readable.
      const anchorIds = parent ? [parent, ...descendants(o, parent)] : items.map((i) => i.id)
      const lastIdx = Math.max(-1, ...anchorIds.map((aid) => items.findIndex((i) => i.id === aid)))
      items.splice(lastIdx + 1, 0, newItem(id, parent))
      return { ...o, items }
    })
    setExpanded((prev) => new Set(prev).add(id))
    setTab('items')
  }

  function removeItem(id: string) {
    edit((o) => {
      const gone = new Set([id, ...descendants(o, id)])
      return {
        ...o,
        items: o.items
          .filter((i) => !gone.has(i.id))
          .map((i) => ({ ...i, depends_on: i.depends_on.filter((d) => !gone.has(d)) })),
      }
    })
  }

  function moveItem(id: string, dir: -1 | 1) {
    edit((o) => {
      const item = o.items.find((i) => i.id === id)
      if (!item) return o
      const siblings = childrenOf(o, item.parent_id ?? null)
      const pos = siblings.findIndex((s) => s.id === id)
      const other = siblings[pos + dir]
      if (!other) return o
      const items = [...o.items]
      const a = items.findIndex((i) => i.id === id)
      const b = items.findIndex((i) => i.id === other.id)
      ;[items[a], items[b]] = [items[b], items[a]]
      return { ...o, items }
    })
  }

  function toggleExpanded(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      setMessage(t('obj.copied'))
    } catch (err) {
      setMessage(String(err))
    }
  }

  const statusLabel = (s: ObjectiveStatus) => t(`obj.status.${s}` as MessageKey)
  const itemLabel = (s: ObjectiveItemStatus) => t(`obj.item.${s}` as MessageKey)

  const selectedConnection = connections.find((c) => c.id === draft?.connection_id)
  const effectiveConnection =
    selectedConnection ?? connections.find(isClaudeConnection) ?? connections[0]
  const models = connectionModels(effectiveConnection)
  const running = draft?.status === 'running'
  const p = draft?.progress
  const waitingManual =
    draft?.status === 'paused' &&
    draft.items.some((i) => i.kind === 'manual' && i.status === 'pending' && isLeaf(draft, i.id))
  const waitingTasks =
    draft?.status === 'paused' && draft.items.some((i) => i.kind === 'task' && i.status === 'running')

  const logNewestFirst = useMemo(() => [...(draft?.log ?? [])].reverse(), [draft?.log])

  // ─── Item tree rendering ───────────────────────────────────────────────────

  function renderItem(item: ObjectiveItem, o: Objective, depth: number): ReactNode {
    const leaf = isLeaf(o, item.id)
    const kids = childrenOf(o, item.id)
    const isOpen = expanded.has(item.id)
    const path = itemPath(o, item.id)
    const deps = item.depends_on.map((d) => itemPath(o, d)).filter(Boolean)
    const canSplit = depth + 1 < MAX_DEPTH && item.status !== 'running'
    const itemBusy = busy?.endsWith(`:${item.id}`)
    return (
      <li key={item.id} className={`obj-item obj-item--${item.status}`}>
        <div className="obj-item__row">
          <span className={`obj-dot obj-dot--${item.status}`} title={itemLabel(item.status)}>
            {item.status === 'running' ? (
              <span className="spinner spinner--xs" />
            ) : item.status === 'done' ? (
              <IconCheck size={11} />
            ) : item.status === 'failed' ? (
              <IconAlert size={11} />
            ) : item.status === 'skipped' ? (
              <IconForward size={11} />
            ) : null}
          </span>
          <span className="obj-item__path">{path}</span>
          <button type="button" className="obj-item__title" onClick={() => toggleExpanded(item.id)}>
            {item.title || <em className="muted">{t('obj.itemTitlePlaceholder')}</em>}
          </button>
          {leaf && (
            <span className={`obj-kind obj-kind--${item.kind}`} title={t(`obj.kindHint.${item.kind}` as MessageKey)}>
              {KIND_ICON[item.kind]}
              {t(`obj.kind.${item.kind}` as MessageKey)}
            </span>
          )}
          {item.kind === 'task' && item.external_status && (
            <span className={`obj-ext obj-ext--${item.external_status}`} title="ProjectHub">
              {t(`editor.ph.status.${item.external_status}` as MessageKey)}
            </span>
          )}
          {item.external_url && (
            <a className="obj-ext obj-ext--link" href={item.external_url} target="_blank" rel="noreferrer">
              GitHub
            </a>
          )}
          {deps.length > 0 && <span className="obj-item__deps">⟵ {deps.join(', ')}</span>}
          <span className="obj-item__meta">
            {item.status === 'running'
              ? elapsed(item.started_at)
              : item.finished_at
                ? elapsed(item.started_at, item.finished_at)
                : ''}
            {item.attempts > 1 ? ` · ${t('obj.attempt', { n: item.attempts })}` : ''}
          </span>
          <div className="obj-item__actions">
            {itemBusy && <span className="spinner spinner--xs" />}
            {leaf && item.kind !== 'manual' && item.status !== 'running' && (
              <button
                type="button"
                className="icon-btn icon-btn--sm"
                disabled={Boolean(busy) || running}
                onClick={() => void run({ item_id: item.id })}
                data-tip={t('obj.runItem')}
                aria-label={t('obj.runItem')}
              >
                <IconPlay size={12} />
              </button>
            )}
            {leaf &&
              item.status !== 'done' &&
              (item.kind === 'task' || item.status !== 'running') &&
              (item.kind === 'manual' || item.kind === 'task' || item.status === 'failed') && (
              <button
                type="button"
                className="icon-btn icon-btn--sm"
                disabled={Boolean(busy)}
                onClick={() => void setItemStatus(item.id, 'done')}
                data-tip={t('obj.markDone')}
                aria-label={t('obj.markDone')}
              >
                <IconCheck size={12} />
              </button>
            )}
            {leaf && item.kind !== 'manual' && item.kind !== 'task' && (
              <button
                type="button"
                className="icon-btn icon-btn--sm"
                disabled={Boolean(busy) || (item.kind === 'workflow' && !item.workflow_id)}
                onClick={() => void openScenario(item)}
                data-tip={t('obj.openScenario')}
                aria-label={t('obj.openScenario')}
              >
                <IconExternal size={12} />
              </button>
            )}
            {canSplit && (
              <button
                type="button"
                className="icon-btn icon-btn--sm obj-ai-btn"
                disabled={Boolean(busy) || !item.title.trim()}
                onClick={() => void assist('decompose', item.id)}
                data-tip={t('obj.aiSplitItem')}
                aria-label={t('obj.aiSplitItem')}
              >
                <IconSplit size={12} />
              </button>
            )}
            {depth + 1 < MAX_DEPTH && (
              <button
                type="button"
                className="icon-btn icon-btn--sm"
                disabled={item.status === 'running'}
                onClick={() => addItem(item.id)}
                data-tip={t('obj.addSubItem')}
                aria-label={t('obj.addSubItem')}
              >
                <IconPlus size={12} />
              </button>
            )}
            <button
              type="button"
              className={`icon-btn icon-btn--sm${isOpen ? ' is-active' : ''}`}
              onClick={() => toggleExpanded(item.id)}
              data-tip={t('obj.edit')}
              aria-label={t('obj.edit')}
              aria-expanded={isOpen}
            >
              <IconChevron size={12} className={isOpen ? '' : 'rot--90'} />
            </button>
          </div>
        </div>

        {item.status === 'failed' && item.error && !isOpen && (
          <div className="obj-item__error">
            <IconAlert size={12} /> {item.error}
          </div>
        )}
        {item.result && !isOpen && leaf && (
          <button type="button" className="obj-item__preview" onClick={() => toggleExpanded(item.id)}>
            {item.result.replace(/[#*`|>]/g, '').slice(0, 220)}
            {item.result.length > 220 ? '…' : ''}
          </button>
        )}

        {isOpen && renderEditor(item, o, leaf)}

        {kids.length > 0 && <ul className="obj-tree">{kids.map((k) => renderItem(k, o, depth + 1))}</ul>}
      </li>
    )
  }

  function renderEditor(item: ObjectiveItem, o: Objective, leaf: boolean): ReactNode {
    const blocked = new Set([...dependents(o, item.id), ...ancestors(o, item.id)])
    const depOptions = orderedIds(o).filter((id) => !blocked.has(id))
    const locked = item.status === 'running'
    return (
      <div className="obj-editor">
        <div className="form__row">
          <Field label={t('obj.itemTitle')}>
            <input
              value={item.title}
              disabled={locked}
              onChange={(e) => editItem(item.id, { title: e.target.value })}
              placeholder={t('obj.itemTitlePlaceholder')}
              autoFocus={!item.title}
            />
          </Field>
        </div>
        {leaf && (
          <Field label={t('obj.itemKind')} hint={t(`obj.kindHint.${item.kind}` as MessageKey)}>
            <Segmented
              value={item.kind}
              onChange={(kind) => editItem(item.id, { kind })}
              options={KINDS.filter(
                (k) => k !== 'task' || item.kind === 'task' || (projecthubReady && Boolean(o.project_hub_project_id)),
              ).map((k) => ({
                value: k,
                label: t(`obj.kind.${k}` as MessageKey),
                icon: KIND_ICON[k],
              }))}
            />
          </Field>
        )}
        <Field label={t('obj.itemDescription')}>
          <textarea
            rows={2}
            value={item.description}
            disabled={locked}
            onChange={(e) => editItem(item.id, { description: e.target.value })}
          />
        </Field>
        {leaf && (item.kind === 'ai' || item.kind === 'research') && (
          <Field label={t('obj.itemPrompt')}>
            <textarea
              rows={2}
              value={item.prompt ?? ''}
              disabled={locked}
              placeholder={t('obj.itemPromptPlaceholder')}
              onChange={(e) => editItem(item.id, { prompt: e.target.value || null })}
            />
          </Field>
        )}
        {leaf && item.kind === 'research' && (
          <Field label={t('obj.itemQuery')}>
            <input
              value={item.query ?? ''}
              disabled={locked}
              placeholder={item.title}
              onChange={(e) => editItem(item.id, { query: e.target.value || null })}
            />
          </Field>
        )}
        {leaf && item.kind === 'workflow' && (
          <Field label={t('obj.itemScenario')}>
            <select
              value={item.workflow_id ?? ''}
              disabled={locked || Boolean(busy)}
              onChange={(e) => void linkWorkflow(item.id, e.target.value)}
            >
              <option value="">{t('obj.itemScenarioPick')}</option>
              {workflows.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.name}
                </option>
              ))}
            </select>
          </Field>
        )}
        {depOptions.length > 0 && (
          <Field label={t('obj.itemDeps')}>
            <div className="chip-row">
              {depOptions.map((id) => {
                const on = item.depends_on.includes(id)
                const dep = o.items.find((i) => i.id === id)
                return (
                  <button
                    key={id}
                    type="button"
                    disabled={locked}
                    className={`filter-chip filter-chip--sm${on ? ' is-active' : ''}`}
                    title={dep?.title}
                    onClick={() =>
                      editItem(item.id, {
                        depends_on: on ? item.depends_on.filter((d) => d !== id) : [...item.depends_on, id],
                      })
                    }
                  >
                    {itemPath(o, id)} {dep?.title.slice(0, 24)}
                  </button>
                )
              })}
            </div>
          </Field>
        )}

        {item.error && (
          <div className="callout callout--error">
            <IconAlert size={14} />
            <div>
              <strong>{t('obj.itemError')}</strong>
              <div className="mono obj-pre">{item.error}</div>
            </div>
          </div>
        )}
        {item.result && (
          <div className="obj-result">
            <div className="obj-result__head">
              <span className="eyebrow">{t('obj.itemResult')}</span>
              {item.finished_at && <span className="muted">{formatDateTime(item.finished_at, locale)}</span>}
              <button type="button" className="btn btn--ghost btn--sm" onClick={() => void copy(item.result ?? '')}>
                <IconCopy size={12} />
                {t('obj.copy')}
              </button>
            </div>
            <MiniMarkdown text={item.result} />
          </div>
        )}

        <div className="form__actions">
          {leaf && (item.status !== 'running' || item.kind === 'task') && (
            <>
              {item.status !== 'done' && (
                <button type="button" className="btn btn--sm" disabled={Boolean(busy)} onClick={() => void setItemStatus(item.id, 'done')}>
                  <IconCheck size={12} />
                  {t('obj.markDone')}
                </button>
              )}
              {item.status !== 'skipped' && item.status !== 'done' && (
                <button type="button" className="btn btn--sm" disabled={Boolean(busy)} onClick={() => void setItemStatus(item.id, 'skipped')}>
                  <IconForward size={12} />
                  {t('obj.skip')}
                </button>
              )}
              {item.status !== 'pending' && (
                <button type="button" className="btn btn--sm" disabled={Boolean(busy)} onClick={() => void setItemStatus(item.id, 'pending')}>
                  <IconRefresh size={12} />
                  {t('obj.resetItem')}
                </button>
              )}
            </>
          )}
          <div className="toolbar__spacer" />
          <button type="button" className="icon-btn icon-btn--sm" onClick={() => moveItem(item.id, -1)} data-tip={t('obj.moveUp')} aria-label={t('obj.moveUp')}>
            <IconChevron size={12} className="rot-180" />
          </button>
          <button type="button" className="icon-btn icon-btn--sm" onClick={() => moveItem(item.id, 1)} data-tip={t('obj.moveDown')} aria-label={t('obj.moveDown')}>
            <IconChevron size={12} />
          </button>
          <button
            type="button"
            className="icon-btn icon-btn--sm icon-btn--danger"
            disabled={locked}
            onClick={() => removeItem(item.id)}
            data-tip={t('obj.removeItem')}
            aria-label={t('obj.removeItem')}
          >
            <IconTrash size={12} />
          </button>
        </div>
      </div>
    )
  }

  // ─── Layout ────────────────────────────────────────────────────────────────

  const tabs = draft
    ? [
        { id: 'items' as const, label: t('obj.tab.items'), icon: <IconListTree size={14} />, count: p?.total ?? 0 },
        { id: 'setup' as const, label: t('obj.tab.setup'), icon: <IconSettings size={14} /> },
        { id: 'log' as const, label: t('obj.tab.log'), icon: <IconActivity size={14} />, count: draft.log.length },
        {
          id: 'summary' as const,
          label: t('obj.tab.summary'),
          icon: <IconFlag size={14} />,
          alert: draft.achieved === false,
        },
      ]
    : []

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      flush
      icon={<IconFlag size={18} />}
      title={t('obj.title')}
      subtitle={t('obj.hint')}
    >
      <div className="mission objectives">
        <aside className="mission__sidebar">
          <form
            className="obj-new"
            onSubmit={(e) => {
              e.preventDefault()
              void create(true)
            }}
          >
            <span className="eyebrow">{t('obj.new')}</span>
            <textarea
              rows={3}
              value={newText}
              onChange={(e) => setNewText(e.target.value)}
              placeholder={t('obj.newPlaceholder')}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault()
                  void create(true)
                }
              }}
            />
            <div className="obj-new__actions">
              <button type="submit" className="btn btn--primary btn--sm" disabled={!newText.trim() || Boolean(busy)}>
                {busy === 'create-ai' ? <span className="spinner spinner--xs" /> : <IconSparkle size={13} />}
                {t('obj.createAi')}
              </button>
              <button type="button" className="btn btn--sm" disabled={!newText.trim() || Boolean(busy)} onClick={() => void create(false)}>
                {t('obj.create')}
              </button>
            </div>
          </form>

          <span className="eyebrow">
            {t('obj.list')} {live && <span className="obj-live" title={t('obj.live')} />}
          </span>
          {list.length === 0 && <p className="mission__empty">{t('obj.empty')}</p>}
          <ul className="mission__list">
            {list.map((o) => (
              <li key={o.id}>
                <button
                  type="button"
                  className={`mission-item${o.id === selectedId ? ' is-active' : ''}`}
                  onClick={() => select(o.id)}
                >
                  <span className="mission-item__top">
                    <strong>{o.title}</strong>
                    <Badge tone={STATUS_TONE[o.status]} dot>
                      {statusLabel(o.status)}
                    </Badge>
                  </span>
                  <Progress value={o.progress.percent} tone={o.status === 'done' ? 'success' : 'brand'} />
                  <span className="mission-item__meta">
                    {t('obj.itemsDone', { done: o.progress.done, total: o.progress.total })}
                    {o.progress.failed > 0 ? ` · ${t('obj.failedCount')}: ${o.progress.failed}` : ''}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </aside>

        <section className="mission__main">
          <Notice text={message} onDismiss={() => setMessage(null)} />

          {!draft ? (
            <EmptyState icon={<IconTarget size={24} />} title={t('obj.pick')} hint={t('obj.pickHint')} />
          ) : (
            <>
              <div className="obj-hero">
                <div className="obj-hero__top">
                  <input
                    className="obj-hero__title"
                    value={draft.title}
                    onChange={(e) => edit((o) => ({ ...o, title: e.target.value }))}
                    aria-label={t('obj.goalTitle')}
                  />
                  <Badge tone={STATUS_TONE[draft.status]} dot>
                    {statusLabel(draft.status)}
                  </Badge>
                </div>
                {draft.description && <p className="mission-hero__text">{draft.description}</p>}
                {(draft.characteristics.length > 0 || draft.deadline || draft.project_hub_project_name) && (
                  <div className="kv-chips">
                    {draft.project_hub_project_name && (
                      <span className="kv-chips__channel">
                        <IconFolder size={12} /> <b>{draft.project_hub_project_name}</b>
                      </span>
                    )}
                    {draft.deadline && (
                      <span className="kv-chips__channel">
                        {t('obj.deadline')}{' '}
                        <b>{new Date(draft.deadline).toLocaleDateString(locale, { timeZone: 'UTC' })}</b>
                      </span>
                    )}
                    {draft.characteristics.map((c, idx) => (
                      <span key={`${c.key}-${idx}`}>
                        {c.key} <b>{c.value}{c.unit ? ` ${c.unit}` : ''}</b>
                      </span>
                    ))}
                  </div>
                )}

                <div className="obj-progress">
                  <Progress value={p?.percent ?? 0} tone={draft.status === 'done' ? 'success' : 'brand'} />
                  <div className="obj-progress__stats">
                    <strong>{Math.round(p?.percent ?? 0)}%</strong>
                    <span>{t('obj.itemsDone', { done: p?.done ?? 0, total: p?.total ?? 0 })}</span>
                    {(p?.running ?? 0) > 0 && <Badge tone="ai">{t('obj.runningCount')}: {p?.running}</Badge>}
                    {(p?.failed ?? 0) > 0 && <Badge tone="error">{t('obj.failedCount')}: {p?.failed}</Badge>}
                    {(p?.pending ?? 0) > 0 && <Badge>{t('obj.pendingCount')}: {p?.pending}</Badge>}
                    {draft.started_at && (
                      <span className="muted">
                        {t('obj.started')} {formatRelative(draft.started_at, locale)}
                        {running ? ` · ${elapsed(draft.started_at)}` : ''}
                      </span>
                    )}
                  </div>
                </div>

                <div className="mission-hero__actions">
                  {running ? (
                    <button type="button" className="btn" disabled={busy === 'stop'} onClick={() => void stop()}>
                      <IconPause size={14} />
                      {t('obj.stop')}
                    </button>
                  ) : draft.status === 'done' ? null : (
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={Boolean(busy) || draft.items.length === 0}
                      onClick={() => void run()}
                    >
                      {busy === 'run' ? <span className="spinner spinner--xs" /> : <IconPlay size={14} />}
                      {draft.status === 'paused' || (p?.done ?? 0) > 0 ? t('obj.continue') : t('obj.run')}
                    </button>
                  )}
                  {!running && (p?.failed ?? 0) > 0 && (
                    <button type="button" className="btn" disabled={Boolean(busy)} onClick={() => void run({ retry_failed: true })}>
                      <IconRefresh size={14} />
                      {t('obj.retry')}
                    </button>
                  )}
                  {!running && draft.status === 'done' && draft.achieved === false && (
                    <button type="button" className="btn obj-ai-btn" disabled={Boolean(busy)} onClick={() => void refine()}>
                      {busy === 'refine' ? <span className="spinner spinner--xs" /> : <IconSparkle size={14} />}
                      {t('obj.refine')}
                    </button>
                  )}
                  {!running && ((p?.done ?? 0) > 0 || draft.status === 'done') && (
                    <button type="button" className="btn btn--ghost" disabled={Boolean(busy)} onClick={() => void run({ reset: true })}>
                      <IconRefresh size={14} />
                      {t('obj.rerun')}
                    </button>
                  )}
                  <span className="obj-sep" />
                  <button type="button" className="btn obj-ai-btn" disabled={Boolean(busy) || running} onClick={() => void assist('fill')}>
                    {busy === 'fill' ? <span className="spinner spinner--xs" /> : <IconSparkle size={14} />}
                    {t('obj.aiFill')}
                  </button>
                  <button type="button" className="btn obj-ai-btn" disabled={Boolean(busy) || running} onClick={() => void assist('decompose')}>
                    {busy === 'decompose' ? <span className="spinner spinner--xs" /> : <IconSplit size={14} />}
                    {t('obj.aiSplit')}
                  </button>
                  <input
                    className="obj-wish"
                    value={wish}
                    onChange={(e) => setWish(e.target.value)}
                    placeholder={t('obj.aiWishPlaceholder')}
                    aria-label={t('obj.aiWish')}
                  />
                  <div className="toolbar__spacer" />
                  <button
                    type="button"
                    className="icon-btn icon-btn--danger"
                    disabled={Boolean(busy) || running}
                    onClick={() => void remove()}
                    data-tip={t('obj.delete')}
                    aria-label={t('obj.delete')}
                  >
                    <IconTrash size={14} />
                  </button>
                </div>

                {busy && ['fill', 'decompose', 'create-ai', 'refine'].includes(busy) && (
                  <div className="callout callout--ai">
                    <span className="spinner spinner--xs" />
                    <span>{t('obj.aiWorking')}</span>
                  </div>
                )}
                {waitingManual && (
                  <div className="callout callout--info">
                    <IconUser size={14} />
                    <span>{t('obj.waitManual')}</span>
                  </div>
                )}
                {waitingTasks && (
                  <div className="callout callout--info">
                    <IconFolder size={14} />
                    <span>{t('obj.waitTasks')}</span>
                  </div>
                )}
                {connections.length === 0 && (
                  <div className="callout callout--muted">
                    <IconAlert size={14} />
                    <span>{t('obj.noAi')}</span>
                    <button type="button" className="btn btn--sm" onClick={onOpenAiSettings}>
                      <IconSettings size={12} />
                      {t('obj.openSettings')}
                    </button>
                  </div>
                )}
                {dirty && (
                  <div className="obj-dirty">
                    <span>{t('obj.unsaved')}</span>
                    <div className="toolbar__spacer" />
                    <button type="button" className="btn btn--ghost btn--sm" onClick={discard}>
                      {t('obj.discard')}
                    </button>
                    <button type="button" className="btn btn--primary btn--sm" disabled={busy === 'save'} onClick={() => void save()}>
                      <IconSave size={12} />
                      {t('obj.save')}
                    </button>
                  </div>
                )}
              </div>

              <Tabs items={tabs} value={tab} onChange={setTab} />

              <div className="mission__tab">
                {tab === 'items' && (
                  <>
                    {draft.items.length === 0 ? (
                      <EmptyState
                        compact
                        icon={<IconListTree size={20} />}
                        title={t('obj.noItems')}
                        hint={t('obj.noItemsHint')}
                        action={
                          <div className="obj-row-actions">
                            <button type="button" className="btn obj-ai-btn" disabled={Boolean(busy)} onClick={() => void assist('decompose')}>
                              <IconSplit size={14} />
                              {t('obj.aiSplit')}
                            </button>
                            <button type="button" className="btn" onClick={() => addItem(null)}>
                              <IconPlus size={14} />
                              {t('obj.addItem')}
                            </button>
                          </div>
                        }
                      />
                    ) : (
                      <ul className="obj-tree obj-tree--root">
                        {childrenOf(draft, null).map((i) => renderItem(i, draft, 0))}
                        {draft.items
                          .filter((i) => i.parent_id && !draft.items.some((x) => x.id === i.parent_id))
                          .map((i) => renderItem(i, draft, 0))}
                      </ul>
                    )}
                    {draft.items.length > 0 && (
                      <button type="button" className="btn btn--ghost btn--sm obj-add" onClick={() => addItem(null)}>
                        <IconPlus size={13} />
                        {t('obj.addItem')}
                      </button>
                    )}
                  </>
                )}

                {tab === 'setup' && (
                  <div className="obj-setup">
                    <Card title={t('obj.goalTitle')}>
                      <div className="form">
                        <Field label={t('obj.description')}>
                          <textarea
                            rows={3}
                            value={draft.description}
                            placeholder={t('obj.descriptionPlaceholder')}
                            onChange={(e) => edit((o) => ({ ...o, description: e.target.value }))}
                          />
                        </Field>
                        <Field label={t('obj.deadline')}>
                          <input
                            type="date"
                            value={draft.deadline ? draft.deadline.slice(0, 10) : ''}
                            onChange={(e) =>
                              edit((o) => ({
                                ...o,
                                // Calendar day, stored as its UTC end (the backend does the same).
                                deadline: e.target.value ? `${e.target.value}T23:59:59Z` : null,
                              }))
                            }
                          />
                        </Field>
                      </div>
                    </Card>

                    <Card title={t('obj.characteristics')} subtitle={t('obj.characteristicsHint')}>
                      <div className="obj-kv">
                        {draft.characteristics.map((c, idx) => (
                          <div key={idx} className="obj-kv__row">
                            <input
                              value={c.key}
                              placeholder={t('obj.charKey')}
                              onChange={(e) =>
                                edit((o) => ({
                                  ...o,
                                  characteristics: o.characteristics.map((x, i) => (i === idx ? { ...x, key: e.target.value } : x)),
                                }))
                              }
                            />
                            <input
                              value={c.value}
                              placeholder={t('obj.charValue')}
                              onChange={(e) =>
                                edit((o) => ({
                                  ...o,
                                  characteristics: o.characteristics.map((x, i) => (i === idx ? { ...x, value: e.target.value } : x)),
                                }))
                              }
                            />
                            <input
                              className="obj-kv__unit"
                              value={c.unit ?? ''}
                              placeholder={t('obj.charUnit')}
                              onChange={(e) =>
                                edit((o) => ({
                                  ...o,
                                  characteristics: o.characteristics.map((x, i) =>
                                    i === idx ? { ...x, unit: e.target.value || null } : x,
                                  ),
                                }))
                              }
                            />
                            <button
                              type="button"
                              className="icon-btn icon-btn--sm icon-btn--danger"
                              onClick={() => edit((o) => ({ ...o, characteristics: o.characteristics.filter((_, i) => i !== idx) }))}
                              aria-label={t('obj.removeItem')}
                            >
                              <IconTrash size={12} />
                            </button>
                          </div>
                        ))}
                        <button
                          type="button"
                          className="btn btn--ghost btn--sm"
                          onClick={() => edit((o) => ({ ...o, characteristics: [...o.characteristics, { key: '', value: '' }] }))}
                        >
                          <IconPlus size={12} />
                          {t('obj.addChar')}
                        </button>
                      </div>
                    </Card>

                    <Card title={t('obj.criteria')} subtitle={t('obj.criteriaHint')}>
                      <div className="obj-kv">
                        {draft.success_criteria.map((c, idx) => (
                          <div key={idx} className="obj-kv__row obj-kv__row--single">
                            <span className="obj-kv__num">{idx + 1}</span>
                            <input
                              value={c}
                              placeholder={t('obj.criterionPlaceholder')}
                              onChange={(e) =>
                                edit((o) => ({
                                  ...o,
                                  success_criteria: o.success_criteria.map((x, i) => (i === idx ? e.target.value : x)),
                                }))
                              }
                            />
                            <button
                              type="button"
                              className="icon-btn icon-btn--sm icon-btn--danger"
                              onClick={() => edit((o) => ({ ...o, success_criteria: o.success_criteria.filter((_, i) => i !== idx) }))}
                              aria-label={t('obj.removeItem')}
                            >
                              <IconTrash size={12} />
                            </button>
                          </div>
                        ))}
                        <button
                          type="button"
                          className="btn btn--ghost btn--sm"
                          onClick={() => edit((o) => ({ ...o, success_criteria: [...o.success_criteria, ''] }))}
                        >
                          <IconPlus size={12} />
                          {t('obj.addCriterion')}
                        </button>
                      </div>
                    </Card>

                    <Card title={t('obj.project')} subtitle={t('obj.projectHint')}>
                      {projecthubReady ? (
                        <Field label={t('obj.projectPick')}>
                          <select
                            value={draft.project_hub_project_id ?? ''}
                            onChange={(e) => {
                              const p = phProjects.find((x) => x.id === e.target.value)
                              edit((o) => ({
                                ...o,
                                project_hub_project_id: p?.id ?? null,
                                project_hub_project_name: p?.name ?? null,
                              }))
                            }}
                          >
                            <option value="">{t('obj.projectNone')}</option>
                            {draft.project_hub_project_id &&
                              !phProjects.some((p) => p.id === draft.project_hub_project_id) && (
                                <option value={draft.project_hub_project_id}>
                                  {draft.project_hub_project_name ?? draft.project_hub_project_id}
                                </option>
                              )}
                            {phProjects.map((p) => (
                              <option key={p.id} value={p.id}>
                                {p.name} — {p.root_path}
                              </option>
                            ))}
                          </select>
                        </Field>
                      ) : (
                        <div className="callout callout--muted">
                          <IconFolder size={14} />
                          <span>{t('obj.projectInstall')}</span>
                          {onOpenProjectHub && (
                            <button type="button" className="btn btn--sm" onClick={onOpenProjectHub}>
                              {t('nav.projecthub')}
                            </button>
                          )}
                        </div>
                      )}
                    </Card>

                    <Card title={t('obj.ai')} subtitle={t('obj.aiHint')}>
                      <div className="form">
                        <div className="form__row">
                          <Field label={t('obj.connection')}>
                            <select
                              value={draft.connection_id ?? ''}
                              onChange={(e) => edit((o) => ({ ...o, connection_id: e.target.value || null, model: null }))}
                            >
                              <option value="">
                                {t('obj.connectionAuto')}
                                {effectiveConnection && !draft.connection_id ? ` · ${effectiveConnection.name}` : ''}
                              </option>
                              {connections.map((c) => (
                                <option key={c.id} value={c.id}>
                                  {c.name}
                                  {c.config?.default_model ? ` · ${String(c.config.default_model)}` : ''}
                                </option>
                              ))}
                            </select>
                          </Field>
                          <Field label={t('obj.model')}>
                            {models.length > 0 ? (
                              <select value={draft.model ?? ''} onChange={(e) => edit((o) => ({ ...o, model: e.target.value || null }))}>
                                <option value="">
                                  {t('obj.modelDefault')}
                                  {effectiveConnection?.config?.default_model ? ` · ${String(effectiveConnection.config.default_model)}` : ''}
                                </option>
                                {models.map((m) => (
                                  <option key={m} value={m}>
                                    {m}
                                  </option>
                                ))}
                              </select>
                            ) : (
                              <input
                                value={draft.model ?? ''}
                                spellCheck={false}
                                placeholder={String(effectiveConnection?.config?.default_model ?? '')}
                                onChange={(e) => edit((o) => ({ ...o, model: e.target.value || null }))}
                              />
                            )}
                          </Field>
                        </div>
                        <Field label={t('obj.autoRefine')} hint={t('obj.autoRefineHint')}>
                          <Segmented
                            value={String(draft.auto_refine ?? 0)}
                            onChange={(v) => edit((o) => ({ ...o, auto_refine: Number(v) }))}
                            options={['0', '1', '2', '3'].map((v) => ({
                              value: v,
                              label: v === '0' ? t('obj.autoRefineOff') : v,
                            }))}
                          />
                        </Field>
                        {connections.length === 0 && (
                          <button type="button" className="btn btn--sm" onClick={onOpenAiSettings}>
                            <IconSettings size={12} />
                            {t('obj.openSettings')}
                          </button>
                        )}
                      </div>
                    </Card>
                  </div>
                )}

                {tab === 'log' && (
                  <>
                    {logNewestFirst.length === 0 ? (
                      <EmptyState compact icon={<IconActivity size={20} />} title={t('obj.logEmpty')} />
                    ) : (
                      <ul className="obj-log">
                        {logNewestFirst.map((entry, idx) => (
                          <li key={`${entry.at}-${idx}`} className={`obj-log__entry obj-log__entry--${entry.level}`}>
                            <time title={formatDateTime(entry.at, locale)}>{formatTime(entry.at, locale)}</time>
                            {entry.item_id && draft.items.some((i) => i.id === entry.item_id) && (
                              <button
                                type="button"
                                className="chip chip--mono chip--btn"
                                onClick={() => {
                                  setTab('items')
                                  setExpanded((prev) => new Set(prev).add(entry.item_id!))
                                }}
                              >
                                {itemPath(draft, entry.item_id)}
                              </button>
                            )}
                            <span>{entry.message}</span>
                          </li>
                        ))}
                      </ul>
                    )}
                  </>
                )}

                {tab === 'summary' && (
                  <div className="obj-summary">
                    {draft.summary ? (
                      <Card
                        title={
                          <span className="obj-verdict">
                            {draft.achieved === false ? <IconAlert size={16} /> : <IconCheck size={16} />}
                            {draft.achieved === false ? t('obj.notAchieved') : t('obj.achieved')}
                          </span>
                        }
                        subtitle={draft.finished_at ? `${t('obj.finished')} ${formatDateTime(draft.finished_at, locale)}` : undefined}
                        actions={
                          <button type="button" className="btn btn--ghost btn--sm" onClick={() => void copy(draft.summary ?? '')}>
                            <IconCopy size={12} />
                            {t('obj.copy')}
                          </button>
                        }
                        className={draft.achieved === false ? 'obj-verdict--no' : 'obj-verdict--yes'}
                      >
                        <MiniMarkdown text={draft.summary} />
                        {draft.achieved === false && !running && (
                          <div className="obj-refine">
                            <span>{t('obj.refineHint')}</span>
                            <button type="button" className="btn obj-ai-btn" disabled={Boolean(busy)} onClick={() => void refine()}>
                              {busy === 'refine' ? <span className="spinner spinner--xs" /> : <IconSparkle size={14} />}
                              {t('obj.refine')}
                            </button>
                          </div>
                        )}
                        {draft.refine_rounds > 0 && (
                          <p className="muted">{t('obj.refinedRounds', { n: draft.refine_rounds })}</p>
                        )}
                      </Card>
                    ) : (
                      <EmptyState compact icon={<IconFlag size={20} />} title={t('obj.summaryEmpty')} />
                    )}

                    {draft.items.some((i) => i.result) && (
                      <>
                        <h3 className="section-title">{t('obj.results')}</h3>
                        {orderedIds(draft)
                          .map((id) => draft.items.find((i) => i.id === id)!)
                          .filter((i) => i && i.result && isLeaf(draft, i.id))
                          .map((i) => (
                            <details key={i.id} className="obj-result obj-result--collapsible">
                              <summary>
                                <span className="obj-item__path">{itemPath(draft, i.id)}</span>
                                <strong>{i.title}</strong>
                                <Badge tone={ITEM_TONE[i.status]}>{itemLabel(i.status)}</Badge>
                              </summary>
                              <MiniMarkdown text={i.result ?? ''} />
                            </details>
                          ))}
                      </>
                    )}
                    {dirty && (
                      <p className="muted">
                        <IconEdit size={12} /> {t('obj.unsaved')}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
        </section>
      </div>
    </Modal>
  )
}
