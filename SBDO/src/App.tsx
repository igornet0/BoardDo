import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Edge } from '@xyflow/react'

import { createWorkflow, getWorkflow, runWorkflow, updateWorkflow } from './api/workflows'
import {
  getRuntime,
  listRuntimes,
  reloadRuntime,
  startRuntime,
  stopRuntime,
} from './api/runtime'
import { getExecution, getExecutionNodes } from './api/executions'
import { wsUrl } from './api/client'
import { NodePalette } from './canvas/NodePalette'
import { NodeEditor, WorkflowInspector } from './canvas/NodeEditor'
import {
  WorkflowCanvas,
  createBoardNode,
  demoWorkflow,
  flowToWorkflow,
  nextId,
  workflowToFlow,
} from './canvas/WorkflowCanvas'
import type { BoardNode, BoardNodeData } from './canvas/BoardNode'
import type { PaletteItem } from './canvas/paletteItems'
import { ConnectionsPanel } from './components/ConnectionsPanel'
import { IngressPanel } from './components/IngressPanel'
import { ProjectsPanel } from './components/ProjectsPanel'
import { RuntimePanel } from './components/RuntimePanel'
import { ScenariosPanel } from './components/ScenariosPanel'
import { GoalsPanel } from './components/GoalsPanel'
import { ToolsPanel } from './components/ToolsPanel'
import { SettingsPanel, type SettingsTab } from './components/SettingsPanel'
import { AgentChatPanel } from './components/AgentChatPanel'
import { applyOps } from './agent/applyOps'
import { getAgentSettings, type AgentSettings } from './api/agent'
import type { ScenarioTemplate } from './scenarios'
import { usePreferences } from './settings/PreferencesContext'
import type {
  Execution,
  ExecutionEvent,
  NodeExecution,
  RuntimeSnapshot,
} from './types'
import { TYPE_IDS } from './types'
import { TopBar } from './shell/TopBar'
import { BottomPanel, type BottomTab } from './shell/BottomPanel'
import { CommandPalette, type CommandAction } from './ui/CommandPalette'
import { ActivityRail, type RailTarget } from './shell/ActivityRail'
import { Toast } from './shell/Toast'
import { isModalOpen } from './ui/Modal'
import { PALETTE } from './canvas/paletteItems'
import './App.css'

type Snapshot = { nodes: BoardNode[]; edges: Edge[] }

type Panel =
  | 'projects'
  | 'scenarios'
  | 'goals'
  | 'tools'
  | 'runtime'
  | 'connections'
  | 'ingress'
  | 'settings'

/** Log node output is `{ message: string }` — surface it in the Console tab. */
function extractLogMessage(output: unknown): string | null {
  if (!output || typeof output !== 'object' || Array.isArray(output)) return null
  const message = (output as { message?: unknown }).message
  return typeof message === 'string' ? message : null
}

export default function App() {
  const { t } = usePreferences()
  const demo = useMemo(() => demoWorkflow(), [])
  const [workflowId, setWorkflowId] = useState<string | null>(null)
  const [workflowName, setWorkflowName] = useState('Amount Gate')
  const [workflowDescription, setWorkflowDescription] = useState(
    'Phase 1 demo workflow',
  )
  const [version, setVersion] = useState(0)
  const [dirty, setDirty] = useState(false)
  const [panel, setPanel] = useState<Panel | null>(null)
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('theme')
  const [showAgent, setShowAgent] = useState(false)
  const [agentSettings, setAgentSettings] = useState<AgentSettings | null>(null)
  const [agentSelectMode, setAgentSelectMode] = useState(false)
  const [agentPinnedIds, setAgentPinnedIds] = useState<string[]>([])
  const [cmdOpen, setCmdOpen] = useState(false)
  const [cmdMode, setCmdMode] = useState<'command' | 'nodes'>('command')
  const [nodes, setNodes] = useState<BoardNode[]>(demo.nodes)
  const [edges, setEdges] = useState<Edge[]>(demo.edges)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [runStatuses, setRunStatuses] = useState<
    Record<string, NonNullable<BoardNodeData['runStatus']>>
  >({})
  const [nodeDurations, setNodeDurations] = useState<Record<string, number>>({})
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [execution, setExecution] = useState<Execution | null>(null)
  const [nodeExecutions, setNodeExecutions] = useState<NodeExecution[]>([])
  const [debugNodeId, setDebugNodeId] = useState<string | null>(null)
  const [runtime, setRuntime] = useState<RuntimeSnapshot | null>(null)
  const [liveRuntimes, setLiveRuntimes] = useState(0)
  const [askText, setAskText] = useState('')
  const [bottomOpen, setBottomOpen] = useState(false)
  const [bottomTab, setBottomTab] = useState<BottomTab>('execution')
  const [consoleLines, setConsoleLines] = useState<string[]>([])
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const [inspectorCollapsed, setInspectorCollapsed] = useState(false)

  const history = useRef<Snapshot[]>([{ nodes: demo.nodes, edges: demo.edges }])
  const historyIndex = useRef(0)
  const skipHistory = useRef(false)

  const pushHistory = useCallback((nextNodes: BoardNode[], nextEdges: Edge[]) => {
    if (skipHistory.current) return
    const snap = { nodes: nextNodes, edges: nextEdges }
    const trimmed = history.current.slice(0, historyIndex.current + 1)
    trimmed.push(snap)
    if (trimmed.length > 50) trimmed.shift()
    history.current = trimmed
    historyIndex.current = trimmed.length - 1
  }, [])

  const setNodesTracked = useCallback(
    (updater: BoardNode[] | ((prev: BoardNode[]) => BoardNode[])) => {
      setNodes((prev) => {
        const next = typeof updater === 'function' ? updater(prev) : updater
        pushHistory(next, edges)
        setDirty(true)
        return next
      })
    },
    [edges, pushHistory],
  )

  const setEdgesTracked = useCallback(
    (updater: Edge[] | ((prev: Edge[]) => Edge[])) => {
      setEdges((prev) => {
        const next = typeof updater === 'function' ? updater(prev) : updater
        pushHistory(nodes, next)
        setDirty(true)
        return next
      })
    },
    [nodes, pushHistory],
  )

  function undo() {
    if (historyIndex.current <= 0) return
    historyIndex.current -= 1
    const snap = history.current[historyIndex.current]
    skipHistory.current = true
    setNodes(snap.nodes)
    setEdges(snap.edges)
    skipHistory.current = false
    setDirty(true)
  }

  function redo() {
    if (historyIndex.current >= history.current.length - 1) return
    historyIndex.current += 1
    const snap = history.current[historyIndex.current]
    skipHistory.current = true
    setNodes(snap.nodes)
    setEdges(snap.edges)
    skipHistory.current = false
    setDirty(true)
  }

  async function refreshRuntime(id: string) {
    try {
      const snap = await getRuntime(id)
      setRuntime(snap)
    } catch {
      setRuntime(null)
    }
  }

  async function refreshLiveCount() {
    try {
      const list = await listRuntimes()
      setLiveRuntimes(list.length)
    } catch {
      /* ignore polling errors */
    }
  }

  useEffect(() => {
    void getAgentSettings()
      .then(setAgentSettings)
      .catch(() => setAgentSettings(null))
  }, [])

  useEffect(() => {
    if (!workflowId) {
      setRuntime(null)
      return
    }
    void refreshRuntime(workflowId)
    const timer = window.setInterval(() => void refreshRuntime(workflowId), 3000)
    return () => window.clearInterval(timer)
  }, [workflowId])

  useEffect(() => {
    void refreshLiveCount()
    const timer = window.setInterval(() => void refreshLiveCount(), 4000)
    return () => window.clearInterval(timer)
  }, [])

  useEffect(() => {
    if (!message) return
    const timer = window.setTimeout(() => setMessage(null), 6000)
    return () => window.clearTimeout(timer)
  }, [message])

  useEffect(() => {
    const socket = new WebSocket(wsUrl())
    socket.onmessage = (ev) => {
      try {
        const event = JSON.parse(ev.data as string) as ExecutionEvent
        if (event.type === 'node.started') {
          setRunStatuses((s) => ({ ...s, [event.node_id]: 'running' }))
          setConsoleLines((lines) => [
            ...lines,
            `> node started  ${event.node_id}`,
          ])
        } else if (event.type === 'node.completed') {
          setRunStatuses((s) => ({ ...s, [event.node_id]: 'completed' }))
          setNodeDurations((d) => ({
            ...d,
            [event.node_id]: event.duration_ms,
          }))
          const logMessage = extractLogMessage(event.output)
          setConsoleLines((lines) => {
            const next = [
              ...lines,
              `> node completed ${event.node_id}  ${event.duration_ms}ms`,
            ]
            if (logMessage != null) {
              next.push(`  log  ${logMessage}`)
            }
            return next
          })
          if (logMessage != null) {
            setBottomOpen(true)
            setBottomTab('console')
          }
        } else if (event.type === 'node.failed') {
          setRunStatuses((s) => ({ ...s, [event.node_id]: 'failed' }))
          setConsoleLines((lines) => [
            ...lines,
            `> node failed    ${event.node_id}  ${event.error}`,
          ])
          setBottomOpen(true)
          setBottomTab('execution')
        } else if (event.type === 'execution.completed') {
          void refreshExecution(event.execution_id)
          if (workflowId) void refreshRuntime(workflowId)
          void refreshLiveCount()
          setConsoleLines((lines) => [
            ...lines,
            `> execution ${event.status}  ${event.execution_id.slice(0, 8)}`,
          ])
          if (event.status === 'failed') {
            setBottomOpen(true)
            setBottomTab('execution')
          }
        }
      } catch {
        /* ignore malformed */
      }
    }
    return () => socket.close()
  }, [workflowId])

  useEffect(() => {
    function onResize() {
      const w = window.innerWidth
      setSidebarOpen(w >= 1100)
      setInspectorCollapsed(w < 1280)
    }
    onResize()
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  useEffect(() => {
    function onKey(ev: KeyboardEvent) {
      const meta = ev.metaKey || ev.ctrlKey
      const target = ev.target as HTMLElement | null
      const typing =
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.isContentEditable)

      if (meta && ev.key.toLowerCase() === 'k') {
        ev.preventDefault()
        if (isModalOpen()) return
        setCmdMode('command')
        setCmdOpen(true)
        return
      }
      // Canvas shortcuts must not fire behind an open dialog.
      if (isModalOpen()) return
      if (meta && ev.key.toLowerCase() === 's') {
        ev.preventDefault()
        void handleSave()
        return
      }
      if (meta && ev.key.toLowerCase() === 'z' && !ev.shiftKey) {
        if (typing) return
        ev.preventDefault()
        undo()
        return
      }
      if (meta && ev.key.toLowerCase() === 'z' && ev.shiftKey) {
        if (typing) return
        ev.preventDefault()
        redo()
        return
      }
      if (ev.key === 'Escape') {
        if (cmdOpen) {
          setCmdOpen(false)
          return
        }
        if (agentSelectMode) {
          setAgentSelectMode(false)
          return
        }
        if (showAgent) {
          setShowAgent(false)
          return
        }
        setSelectedId(null)
        return
      }
      if (typing) return
      if (ev.key.toLowerCase() === 'r' && !meta) {
        ev.preventDefault()
        void handleRun()
      } else if (ev.key.toLowerCase() === 't' && !meta) {
        ev.preventDefault()
        void handleSandboxTest()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cmdOpen, nodes, edges, askText, busy, workflowId, workflowName, agentSelectMode, showAgent])

  async function refreshExecution(id: string) {
    try {
      const [ex, nes] = await Promise.all([
        getExecution(id),
        getExecutionNodes(id),
      ])
      setExecution(ex)
      setNodeExecutions(nes)
      const durations: Record<string, number> = {}
      for (const n of nes) {
        if (n.duration_ms != null) durations[n.node_id] = n.duration_ms
      }
      setNodeDurations(durations)
    } catch (err) {
      setMessage(String(err))
    }
  }

  const selected = nodes.find((n) => n.id === selectedId) ?? null
  const runtimeArmed =
    runtime != null && runtime.state !== 'stopped' && runtime.state !== 'stopping'
  const telegramOnly =
    nodes.some(
      (n) => n.data.typeId === TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
    ) && !nodes.some((n) => n.data.typeId === TYPE_IDS.TRIGGER_MANUAL)

  const addNode = useCallback(
    (item: PaletteItem, position?: { x: number; y: number }) => {
      const node = createBoardNode(item, position)
      setNodesTracked((nds) => [...nds, node])
      setSelectedId(node.id)
      return node
    },
    [setNodesTracked],
  )

  const addConnected = useCallback(
    (
      item: PaletteItem,
      position: { x: number; y: number },
      connection: {
        source: string
        sourceHandle: string | null
        target: string | null
        targetHandle: string | null
      },
    ) => {
      const node = createBoardNode(item, position)
      setNodesTracked((nds) => [...nds, node])
      setEdgesTracked((eds) => [
        ...eds,
        {
          id: nextId('e'),
          source: connection.source,
          target: node.id,
          sourceHandle: connection.sourceHandle,
          targetHandle: connection.targetHandle,
        },
      ])
      setSelectedId(node.id)
    },
    [setNodesTracked, setEdgesTracked],
  )

  function resetCanvas() {
    const fresh = demoWorkflow()
    setWorkflowId(null)
    setWorkflowName('Untitled')
    setWorkflowDescription('')
    setVersion(0)
    setNodes(fresh.nodes)
    setEdges(fresh.edges)
    history.current = [{ nodes: fresh.nodes, edges: fresh.edges }]
    historyIndex.current = 0
    setSelectedId(null)
    setRunStatuses({})
    setNodeDurations({})
    setExecution(null)
    setNodeExecutions([])
    setDebugNodeId(null)
    setRuntime(null)
    setMessage(null)
    setDirty(false)
    setConsoleLines([])
  }

  function loadScenario(scenario: ScenarioTemplate) {
    const flow = workflowToFlow(
      scenario.definition.nodes,
      scenario.definition.edges,
    )
    const name = t(scenario.nameKey)
    setWorkflowId(null)
    setWorkflowName(name)
    setWorkflowDescription(t(scenario.descriptionKey))
    setVersion(0)
    setNodes(flow.nodes)
    setEdges(flow.edges)
    history.current = [{ nodes: flow.nodes, edges: flow.edges }]
    historyIndex.current = 0
    setSelectedId(null)
    setRunStatuses({})
    setNodeDurations({})
    setExecution(null)
    setNodeExecutions([])
    setDebugNodeId(null)
    setRuntime(null)
    setDirty(true)
    const hint = scenario.setupHintKey ? t(scenario.setupHintKey) : null
    setMessage(
      hint
        ? `${t('scenarios.loaded', { name })} — ${hint}`
        : t('scenarios.loaded', { name }),
    )
  }

  async function openWorkflow(id: string) {
    setBusy(true)
    setMessage(null)
    try {
      const wf = await getWorkflow(id)
      const flow = workflowToFlow(wf.definition.nodes, wf.definition.edges)
      setWorkflowId(wf.id)
      setNodes(flow.nodes)
      setEdges(flow.edges)
      history.current = [{ nodes: flow.nodes, edges: flow.edges }]
      historyIndex.current = 0
      setWorkflowName(wf.name)
      setWorkflowDescription(wf.description)
      setVersion(wf.version)
      setSelectedId(null)
      setRunStatuses({})
      setNodeDurations({})
      setExecution(null)
      setNodeExecutions([])
      setDirty(false)
      setMessage(t('projects.opened', { name: wf.name }))
      await refreshRuntime(wf.id)
      await refreshLiveCount()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function handleSave() {
    setBusy(true)
    setMessage(null)
    try {
      const graph = flowToWorkflow(nodes, edges)
      const status = runtimeArmed ? 'active' : 'draft'
      if (!workflowId) {
        const created = await createWorkflow({
          name: workflowName,
          description: workflowDescription,
          nodes: graph.nodes,
          edges: graph.edges,
        })
        setWorkflowId(created.id)
        setVersion(created.version)
        setDirty(false)
        setMessage(t('app.saved', { version: created.version }))
      } else {
        const updated = await updateWorkflow(workflowId, {
          name: workflowName,
          description: workflowDescription,
          status,
          nodes: graph.nodes,
          edges: graph.edges,
        })
        setVersion(updated.version)
        setDirty(false)
        setMessage(t('app.saved', { version: updated.version }))
        await refreshRuntime(workflowId)
      }
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function handleStartRuntime() {
    setBusy(true)
    setMessage(null)
    try {
      const graph = flowToWorkflow(nodes, edges)
      let id = workflowId
      if (!id) {
        const created = await createWorkflow({
          name: workflowName,
          description: workflowDescription,
          nodes: graph.nodes,
          edges: graph.edges,
        })
        id = created.id
        setWorkflowId(created.id)
        setVersion(created.version)
      } else {
        const updated = await updateWorkflow(id, {
          name: workflowName,
          description: workflowDescription,
          status: 'draft',
          nodes: graph.nodes,
          edges: graph.edges,
        })
        setVersion(updated.version)
      }
      const snap = await startRuntime(id)
      setRuntime(snap)
      setDirty(false)
      setMessage(t('runtime.started', { name: snap.workflow_name }))
      await refreshLiveCount()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function handleStopRuntime() {
    if (!workflowId) return
    setBusy(true)
    setMessage(null)
    try {
      const snap = await stopRuntime(workflowId)
      setRuntime(snap)
      setMessage(t('runtime.stopped'))
      await refreshLiveCount()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function handleReloadRuntime() {
    if (!workflowId) return
    setBusy(true)
    setMessage(null)
    try {
      const graph = flowToWorkflow(nodes, edges)
      const updated = await updateWorkflow(workflowId, {
        name: workflowName,
        description: workflowDescription,
        status: 'active',
        nodes: graph.nodes,
        edges: graph.edges,
      })
      setVersion(updated.version)
      const snap = await reloadRuntime(workflowId)
      setRuntime(snap)
      setDirty(false)
      setMessage(t('runtime.reloaded', { name: snap.workflow_name }))
      await refreshLiveCount()
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function persistGraph() {
    const graph = flowToWorkflow(nodes, edges)
    let id = workflowId
    if (!id) {
      const created = await createWorkflow({
        name: workflowName,
        nodes: graph.nodes,
        edges: graph.edges,
      })
      id = created.id
      setWorkflowId(created.id)
      setVersion(created.version)
    } else {
      const updated = await updateWorkflow(id, {
        name: workflowName,
        status: runtimeArmed ? 'active' : 'draft',
        nodes: graph.nodes,
        edges: graph.edges,
      })
      setVersion(updated.version)
    }
    setDirty(false)
    return id
  }

  async function waitForExecution(id: string) {
    for (let i = 0; i < 60; i++) {
      const [ex, nes] = await Promise.all([
        getExecution(id),
        getExecutionNodes(id),
      ])
      setExecution(ex)
      setNodeExecutions(nes)
      const durations: Record<string, number> = {}
      for (const n of nes) {
        if (n.duration_ms != null) durations[n.node_id] = n.duration_ms
      }
      setNodeDurations(durations)
      if (
        ex.status === 'completed' ||
        ex.status === 'failed' ||
        ex.status === 'cancelled'
      ) {
        if (ex.status === 'failed') {
          setBottomOpen(true)
          setBottomTab('execution')
        }
        return
      }
      await new Promise((r) => window.setTimeout(r, 500))
    }
  }

  function sandboxTrigger(text: string) {
    return {
      text,
      sandbox: true,
      source: 'sandbox',
      chat_id: 'sandbox',
      account_id: 'sandbox',
    }
  }

  async function handleRun() {
    setBusy(true)
    setMessage(null)
    setRunStatuses({})
    setNodeDurations({})
    setNodeExecutions([])
    setExecution(null)
    setConsoleLines((lines) => [...lines, '> workflow started'])
    setBottomOpen(true)
    setBottomTab('execution')
    try {
      const id = await persistGraph()
      const trimmed = askText.trim()
      const trigger = !trimmed
        ? {}
        : telegramOnly
          ? sandboxTrigger(trimmed)
          : { text: trimmed }
      const res = await runWorkflow(id, trigger)
      setMessage(t('app.running', { id: res.execution_id.slice(0, 8) }))
      await waitForExecution(res.execution_id)
    } catch (err) {
      setMessage(String(err))
      setBottomOpen(true)
    } finally {
      setBusy(false)
    }
  }

  async function handleSandboxTest() {
    const trimmed = askText.trim()
    if (!trimmed) {
      setMessage(t('sandbox.empty'))
      setBottomOpen(true)
      setBottomTab('test')
      return
    }
    setBusy(true)
    setMessage(null)
    setRunStatuses({})
    setNodeDurations({})
    setNodeExecutions([])
    setExecution(null)
    setBottomOpen(true)
    setBottomTab('test')
    try {
      const id = await persistGraph()
      const res = await runWorkflow(id, sandboxTrigger(trimmed))
      setMessage(t('sandbox.running'))
      await waitForExecution(res.execution_id)
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  function commitGraph(nextNodes: BoardNode[], nextEdges: Edge[]) {
    skipHistory.current = true
    setNodes(nextNodes)
    setEdges(nextEdges)
    skipHistory.current = false
    pushHistory(nextNodes, nextEdges)
    setDirty(true)
  }

  function openSettings(tab: SettingsTab) {
    setSettingsTab(tab)
    setPanel('settings')
  }

  function openRail(target: RailTarget) {
    if (target === 'telegram') openSettings('telegram')
    else if (target === 'settings') openSettings('theme')
    else setPanel(target)
  }

  const railActive: RailTarget | null =
    panel === 'settings' ? (settingsTab === 'telegram' ? 'telegram' : 'settings') : panel

  async function openAgent() {
    try {
      const s = await getAgentSettings()
      setAgentSettings(s)
      if (s.enabled && s.has_connection) {
        setShowAgent(true)
      } else {
        openSettings('agent')
      }
    } catch {
      openSettings('agent')
    }
  }

  function handleCommand(action: CommandAction) {
    switch (action.kind) {
      case 'add-node':
        addNode(action.item)
        break
      case 'run':
        void handleRun()
        break
      case 'test':
        void handleSandboxTest()
        break
      case 'save':
        void handleSave()
        break
      case 'projects':
      case 'scenarios':
      case 'goals':
      case 'tools':
      case 'runtime':
      case 'connections':
      case 'ingress':
        setPanel(action.kind)
        break
      case 'settings':
        openSettings('theme')
        break
      case 'telegram':
        openSettings('telegram')
        break
      case 'new-board':
        resetCanvas()
        break
      case 'toggle-bottom':
        setBottomOpen((v) => !v)
        break
      case 'agent':
        void openAgent()
        break
      case 'agent-settings':
        openSettings('agent')
        break
      default:
        break
    }
  }

  const showInspector = !inspectorCollapsed || selectedId != null
  const triggerCount = nodes.filter((n) => n.data.typeId.startsWith('trigger.')).length

  return (
    <div
      className={`app-shell${bottomOpen ? ' app-shell--bottom-open' : ''}${!sidebarOpen ? ' app-shell--no-sidebar' : ''}${!showInspector ? ' app-shell--no-inspector' : ''}`}
    >
      <TopBar
        workflowName={workflowName}
        onWorkflowNameChange={(name) => {
          setWorkflowName(name)
          setDirty(true)
        }}
        version={version}
        dirty={dirty || version === 0}
        busy={busy}
        canUndo={historyIndex.current > 0}
        canRedo={historyIndex.current < history.current.length - 1}
        runtime={runtime}
        liveRuntimes={liveRuntimes}
        onSave={() => void handleSave()}
        onRun={() => void handleRun()}
        onTest={() => void handleSandboxTest()}
        onUndo={undo}
        onRedo={redo}
        onOpenCommand={() => {
          setCmdMode('command')
          setCmdOpen(true)
        }}
        onOpenProjects={() => setPanel('projects')}
        onOpenRuntime={() => setPanel('runtime')}
        onStartRuntime={() => void handleStartRuntime()}
        onStopRuntime={() => void handleStopRuntime()}
      />

      <ActivityRail
        active={railActive}
        sidebarOpen={sidebarOpen}
        onToggleSidebar={() => setSidebarOpen((v) => !v)}
        onOpen={openRail}
        liveRuntimes={liveRuntimes}
        agentReady={Boolean(agentSettings?.enabled && agentSettings.has_connection)}
        agentOpen={showAgent}
        onOpenAgent={() => (showAgent ? setShowAgent(false) : void openAgent())}
      />

      <div className="workspace">
        {sidebarOpen && (
          <NodePalette
            onAdd={(item) => addNode(item)}
            onClose={() => setSidebarOpen(false)}
            onOpenSearch={() => {
              setCmdMode('nodes')
              setCmdOpen(true)
            }}
          />
        )}
        <WorkflowCanvas
          nodes={nodes}
          edges={edges}
          selectedId={selectedId}
          runStatuses={runStatuses}
          nodeDurations={nodeDurations}
          onNodesChange={(n) => setNodesTracked(n)}
          onEdgesChange={(e) => setEdgesTracked(e)}
          onSelect={(id) => {
            if (agentSelectMode && id) {
              setAgentPinnedIds((ids) =>
                ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id],
              )
            }
            setSelectedId(id)
            if (id && inspectorCollapsed) setInspectorCollapsed(false)
          }}
          onAddAt={(item, pos) => addNode(item, pos)}
          onAddConnected={addConnected}
          pinnedIds={agentPinnedIds}
          selectMode={agentSelectMode}
        />
        {showInspector &&
          (selected ? (
            <NodeEditor
              nodeId={selected.id}
              data={selected.data}
              onClose={() => setSelectedId(null)}
              onApply={(id, config) => {
                setNodesTracked((nds) =>
                  nds.map((n) =>
                    n.id === id ? { ...n, data: { ...n.data, config } } : n,
                  ),
                )
              }}
              onDelete={(id) => {
                setNodesTracked((nds) => nds.filter((n) => n.id !== id))
                setEdgesTracked((eds) =>
                  eds.filter((e) => e.source !== id && e.target !== id),
                )
                setSelectedId(null)
              }}
              onDuplicate={(id) => {
                const src = nodes.find((n) => n.id === id)
                if (!src) return
                const copy: BoardNode = {
                  ...src,
                  id: nextId('node'),
                  position: {
                    x: src.position.x + 40,
                    y: src.position.y + 40,
                  },
                  data: {
                    ...src.data,
                    config: structuredClone(src.data.config),
                    runStatus: 'idle',
                  },
                }
                setNodesTracked((nds) => [...nds, copy])
                setSelectedId(copy.id)
              }}
            />
          ) : (
            <WorkflowInspector
              name={workflowName}
              description={workflowDescription}
              version={version}
              nodeCount={nodes.length}
              edgeCount={edges.length}
              triggerCount={triggerCount}
              onNameChange={(v) => {
                setWorkflowName(v)
                setDirty(true)
              }}
              onDescriptionChange={(v) => {
                setWorkflowDescription(v)
                setDirty(true)
              }}
              onAddTrigger={() => {
                const manual = PALETTE.find((p) => p.typeId === TYPE_IDS.TRIGGER_MANUAL)
                if (manual) addNode(manual, { x: 280, y: 20 })
              }}
            />
          ))}
      </div>

      <BottomPanel
        open={bottomOpen}
        tab={bottomTab}
        onTabChange={setBottomTab}
        onToggle={() => setBottomOpen((v) => !v)}
        askText={askText}
        onAskTextChange={setAskText}
        busy={busy}
        onTest={() => void handleSandboxTest()}
        execution={execution}
        nodeExecutions={nodeExecutions}
        boardNodes={nodes}
        edgeCount={edges.length}
        debugNodeId={debugNodeId}
        onSelectDebugNode={setDebugNodeId}
        onSelectCanvasNode={(id) => {
          setSelectedId(id)
          if (inspectorCollapsed) setInspectorCollapsed(false)
        }}
        consoleLines={consoleLines}
        onClearConsole={() => setConsoleLines([])}
      />

      <Toast message={message} onDismiss={() => setMessage(null)} />

      <CommandPalette
        open={cmdOpen}
        mode={cmdMode}
        onClose={() => setCmdOpen(false)}
        onAction={handleCommand}
      />

      <ProjectsPanel
        open={panel === 'projects'}
        currentId={workflowId}
        onClose={() => setPanel(null)}
        onOpen={(id) => void openWorkflow(id)}
        onNew={resetCanvas}
        onOpenScenarios={() => setPanel('scenarios')}
      />

      <ScenariosPanel
        open={panel === 'scenarios'}
        onClose={() => setPanel(null)}
        onUse={loadScenario}
      />

      <GoalsPanel
        open={panel === 'goals'}
        onClose={() => setPanel(null)}
        onOpenWorkflow={(id) => {
          void openWorkflow(id)
          setPanel(null)
        }}
      />

      <ToolsPanel open={panel === 'tools'} onClose={() => setPanel(null)} />

      <RuntimePanel
        open={panel === 'runtime'}
        currentId={workflowId}
        currentName={workflowName}
        canStartCurrent={Boolean(workflowId) || nodes.length > 0}
        currentArmed={runtimeArmed}
        onClose={() => setPanel(null)}
        onOpen={(id) => void openWorkflow(id)}
        onCurrentStart={() => void handleStartRuntime()}
        onCurrentStop={() => void handleStopRuntime()}
        onCurrentReload={() => void handleReloadRuntime()}
        onChanged={() => {
          void refreshLiveCount()
          if (workflowId) void refreshRuntime(workflowId)
        }}
      />

      <ConnectionsPanel open={panel === 'connections'} onClose={() => setPanel(null)} />

      <IngressPanel open={panel === 'ingress'} onClose={() => setPanel(null)} />

      <SettingsPanel
        open={panel === 'settings'}
        tab={settingsTab}
        onClose={() => setPanel(null)}
        onAgentSettingsChange={setAgentSettings}
      />

      <AgentChatPanel
        open={showAgent}
        onClose={() => {
          setShowAgent(false)
          setAgentSelectMode(false)
        }}
        settings={agentSettings}
        workflow={{
          name: workflowName,
          description: workflowDescription,
          ...flowToWorkflow(nodes, edges),
        }}
        pinnedIds={agentPinnedIds}
        pinnedLabels={nodes
          .filter((n) => agentPinnedIds.includes(n.id))
          .map((n) => ({ id: n.id, typeId: n.data.typeId }))}
        selectMode={agentSelectMode}
        onToggleSelectMode={() => setAgentSelectMode((v) => !v)}
        onRemovePin={(id) =>
          setAgentPinnedIds((ids) => ids.filter((x) => x !== id))
        }
        onClearPins={() => setAgentPinnedIds([])}
        onOps={(ops, summary) => {
          const applied = applyOps(ops, nodes, edges, {
            name: workflowName,
            description: workflowDescription,
          })
          commitGraph(applied.nodes, applied.edges)
          setWorkflowName(applied.meta.name)
          setWorkflowDescription(applied.meta.description)
          setMessage(
            summary || t('agent.applied', { count: ops.length }),
          )
        }}
        onOpenSettings={() => openSettings('agent')}
      />
    </div>
  )
}
