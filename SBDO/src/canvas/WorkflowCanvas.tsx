import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  addEdge,
  type Connection,
  type Edge,
  type NodeChange,
  type EdgeChange,
  type OnConnect,
  applyNodeChanges,
  applyEdgeChanges,
  MarkerType,
  ConnectionMode,
} from '@xyflow/react'
import { useCallback, useMemo, useRef, useState } from 'react'
import '@xyflow/react/dist/style.css'

import { BoardNodeView, type BoardNode, type BoardNodeData } from './BoardNode'
import { PALETTE, labelKeyForType, type PaletteItem } from './paletteItems'
import { messages } from '../i18n/messages'
import { TYPE_IDS, type WorkflowEdge, type WorkflowNode } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import { IconForType, IconSearch, visualCategoryForType } from '../ui/icons'

function defaultLabel(typeId: string): string {
  const key = labelKeyForType(typeId)
  return key ? messages.en[key] : typeId
}

const nodeTypes = { board: BoardNodeView }

let idCounter = 1
export function nextId(prefix: string) {
  return `${prefix}_${idCounter++}`
}

export function workflowToFlow(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): { nodes: BoardNode[]; edges: Edge[] } {
  return {
    nodes: nodes.map((n) => ({
      id: n.id,
      type: 'board' as const,
      position: n.position,
      data: {
        typeId: n.type_id,
        label: defaultLabel(n.type_id),
        config: n.config,
        runStatus: 'idle' as const,
      },
    })),
    edges: edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      sourceHandle: e.source_port ?? undefined,
      targetHandle: e.target_port ?? undefined,
    })),
  }
}

export function flowToWorkflow(
  nodes: BoardNode[],
  edges: Edge[],
): { nodes: WorkflowNode[]; edges: WorkflowEdge[] } {
  return {
    nodes: nodes.map((n) => ({
      id: n.id,
      type_id: n.data.typeId,
      category: null,
      position: { x: n.position.x, y: n.position.y },
      config: n.data.config,
    })),
    edges: edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      source_port: e.sourceHandle ?? null,
      target_port: e.targetHandle ?? null,
    })),
  }
}

export function createBoardNode(
  item: PaletteItem,
  position?: { x: number; y: number },
): BoardNode {
  const id =
    item.typeId === TYPE_IDS.TRIGGER_MANUAL ? nextId('trigger') : nextId('node')
  return {
    id,
    type: 'board',
    position: position ?? {
      x: 140 + Math.random() * 220,
      y: 100 + Math.random() * 180,
    },
    data: {
      typeId: item.typeId,
      label: messages.en[item.labelKey],
      config: structuredClone(item.defaultConfig),
      runStatus: 'idle',
    },
  }
}

function paletteItem(typeId: string): PaletteItem {
  const item = PALETTE.find((p) => p.typeId === typeId)
  if (!item) throw new Error(`Unknown palette type: ${typeId}`)
  return item
}

export function demoWorkflow(): { nodes: BoardNode[]; edges: Edge[] } {
  const trigger = createBoardNode(paletteItem(TYPE_IDS.TRIGGER_MANUAL), {
    x: 280,
    y: 40,
  })
  trigger.id = 'trigger_1'
  const setVar = createBoardNode(paletteItem(TYPE_IDS.DATA_SET), {
    x: 280,
    y: 160,
  })
  setVar.id = 'set_1'
  const cond = createBoardNode(paletteItem(TYPE_IDS.LOGIC_CONDITION), {
    x: 280,
    y: 300,
  })
  cond.id = 'cond_1'
  const logTrue = createBoardNode(paletteItem(TYPE_IDS.DEBUG_LOG), {
    x: 120,
    y: 460,
  })
  logTrue.id = 'log_true'
  logTrue.data.config = { message: 'amount > 100 ✓' }
  const logFalse = createBoardNode(paletteItem(TYPE_IDS.DEBUG_LOG), {
    x: 440,
    y: 460,
  })
  logFalse.id = 'log_false'
  logFalse.data.config = { message: 'amount <= 100' }

  return {
    nodes: [trigger, setVar, cond, logTrue, logFalse],
    edges: [
      { id: 'e1', source: 'trigger_1', target: 'set_1' },
      { id: 'e2', source: 'set_1', target: 'cond_1' },
      {
        id: 'e3',
        source: 'cond_1',
        target: 'log_true',
        sourceHandle: 'true',
      },
      {
        id: 'e4',
        source: 'cond_1',
        target: 'log_false',
        sourceHandle: 'false',
      },
    ],
  }
}

interface Props {
  nodes: BoardNode[]
  edges: Edge[]
  selectedId: string | null
  runStatuses: Record<string, NonNullable<BoardNodeData['runStatus']>>
  nodeDurations?: Record<string, number>
  onNodesChange: (nodes: BoardNode[]) => void
  onEdgesChange: (edges: Edge[]) => void
  onSelect: (id: string | null) => void
  onAddAt: (item: PaletteItem, position: { x: number; y: number }) => void
  onAddConnected?: (
    item: PaletteItem,
    position: { x: number; y: number },
    connection: {
      source: string
      sourceHandle: string | null
      target: string | null
      targetHandle: string | null
    },
  ) => void
  pinnedIds?: string[]
  selectMode?: boolean
}

type PendingConnect = {
  from: {
    source: string
    sourceHandle: string | null
    target: string | null
    targetHandle: string | null
  }
  screen: { x: number; y: number }
  flow: { x: number; y: number }
}

export function WorkflowCanvas({
  nodes,
  edges,
  selectedId,
  runStatuses,
  nodeDurations = {},
  onNodesChange,
  onEdgesChange,
  onSelect,
  onAddAt,
  onAddConnected,
  pinnedIds = [],
  selectMode = false,
}: Props) {
  const { t } = usePreferences()
  const rf = useRef<{
    screenToFlowPosition: (p: { x: number; y: number }) => { x: number; y: number }
    fitView: (opts?: object) => void
  } | null>(null)
  const [pending, setPending] = useState<PendingConnect | null>(null)
  const [addQuery, setAddQuery] = useState('')
  const connecting = useRef<{
    source: string | null
    sourceHandle: string | null
  } | null>(null)

  const displayNodes = useMemo(
    () =>
      nodes.map((n) => ({
        ...n,
        selected: n.id === selectedId,
        className: pinnedIds.includes(n.id) ? 'is-pinned' : undefined,
        data: {
          ...n.data,
          runStatus: runStatuses[n.id] ?? 'idle',
          durationMs: nodeDurations[n.id],
        },
      })),
    [nodes, runStatuses, selectedId, nodeDurations, pinnedIds],
  )

  const displayEdges = useMemo(() => {
    return edges.map((e) => {
      const sourceStatus = runStatuses[e.source]
      const targetStatus = runStatuses[e.target]
      let className = 'board-edge'
      let animated = false
      if (sourceStatus === 'running' || targetStatus === 'running') {
        className += ' board-edge--active'
        animated = true
      } else if (sourceStatus === 'failed' || targetStatus === 'failed') {
        className += ' board-edge--error'
      } else if (sourceStatus === 'completed' && targetStatus === 'completed') {
        className += ' board-edge--success'
      }
      return {
        ...e,
        className,
        animated,
        style: { strokeWidth: 1.6 },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          width: 16,
          height: 16,
          color: 'var(--edge-color)',
        },
      }
    })
  }, [edges, runStatuses])

  const handleNodesChange = useCallback(
    (changes: NodeChange<BoardNode>[]) => {
      onNodesChange(applyNodeChanges(changes, nodes))
    },
    [nodes, onNodesChange],
  )

  const handleEdgesChange = useCallback(
    (changes: EdgeChange<Edge>[]) => {
      onEdgesChange(applyEdgeChanges(changes, edges))
    },
    [edges, onEdgesChange],
  )

  const onConnect: OnConnect = useCallback(
    (connection: Connection) => {
      onEdgesChange(addEdge({ ...connection, id: nextId('e') }, edges))
    },
    [edges, onEdgesChange],
  )

  const addFiltered = useMemo(() => {
    const q = addQuery.trim().toLowerCase()
    if (!q) return PALETTE.slice(0, 12)
    return PALETTE.filter((item) => {
      const label = t(item.labelKey).toLowerCase()
      return label.includes(q) || item.typeId.toLowerCase().includes(q)
    }).slice(0, 12)
  }, [addQuery, t])

  return (
    <div
      className={`canvas-host${selectMode ? ' canvas-host--select' : ''}`}
      onDragOver={(e) => {
        e.preventDefault()
        e.dataTransfer.dropEffect = 'move'
      }}
      onDrop={(e) => {
        e.preventDefault()
        const typeId = e.dataTransfer.getData('application/boarddo-node')
        const item = PALETTE.find((p) => p.typeId === typeId)
        if (!item || !rf.current) return
        const position = rf.current.screenToFlowPosition({
          x: e.clientX,
          y: e.clientY,
        })
        onAddAt(item, position)
      }}
    >
      {nodes.length === 0 && (
        <div className="canvas-empty">
          <span className="canvas-empty__icon">
            <IconForType typeId={TYPE_IDS.TRIGGER_MANUAL} size={22} />
          </span>
          <strong>{t('canvas.emptyTitle')}</strong>
          <p>{t('canvas.emptyHint')}</p>
        </div>
      )}
      {selectMode && (
        <div className="canvas-select-banner">{t('agent.selectHint')}</div>
      )}

      <ReactFlow
        nodes={displayNodes}
        edges={displayEdges}
        nodeTypes={nodeTypes}
        onNodesChange={handleNodesChange}
        onEdgesChange={handleEdgesChange}
        onConnect={onConnect}
        connectionMode={ConnectionMode.Loose}
        onConnectStart={(_, params) => {
          connecting.current = {
            source: params.nodeId ?? null,
            sourceHandle: params.handleId ?? null,
          }
        }}
        onConnectEnd={(event) => {
          const conn = connecting.current
          connecting.current = null
          if (!conn?.source || !rf.current) return
          const targetIsPane =
            event instanceof MouseEvent &&
            (event.target as HTMLElement)?.classList?.contains('react-flow__pane')
          if (!targetIsPane) return
          const clientX = event.clientX
          const clientY = event.clientY
          const flow = rf.current.screenToFlowPosition({
            x: clientX,
            y: clientY,
          })
          setPending({
            from: {
              source: conn.source,
              sourceHandle: conn.sourceHandle,
              target: null,
              targetHandle: null,
            },
            screen: { x: clientX, y: clientY },
            flow,
          })
          setAddQuery('')
        }}
        onInit={(instance) => {
          rf.current = instance
        }}
        onNodeClick={(_, node) => onSelect(node.id)}
        onPaneClick={() => {
          onSelect(null)
          setPending(null)
        }}
        fitView
        deleteKeyCode={['Backspace', 'Delete']}
        panOnScroll
        selectionOnDrag
        panOnDrag={[1, 2]}
        defaultEdgeOptions={{
          type: 'default',
        }}
        proOptions={{ hideAttribution: true }}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={20}
          size={1.2}
          color="var(--canvas-dot)"
        />
        <Controls showInteractive={false} />
        <MiniMap
          pannable
          zoomable
          nodeColor={(n) => `var(--${visualCategoryForType((n.data as BoardNodeData).typeId) === 'integration' ? 'action' : visualCategoryForType((n.data as BoardNodeData).typeId)})`}
          nodeBorderRadius={4}
          maskColor="var(--minimap-mask)"
          className="board-minimap"
        />
      </ReactFlow>

      {pending && (
        <div
          className="canvas-add-menu"
          style={{
            left: Math.min(pending.screen.x, window.innerWidth - 280),
            top: Math.min(pending.screen.y, window.innerHeight - 320),
          }}
        >
          <div className="canvas-add-menu__head">
            <strong>{t('canvas.addNode')}</strong>
            <div className="canvas-add-menu__search">
              <IconSearch size={13} />
              <input
                autoFocus
                value={addQuery}
                onChange={(e) => setAddQuery(e.target.value)}
                placeholder={t('palette.search')}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') setPending(null)
                }}
              />
            </div>
          </div>
          <div className="canvas-add-menu__list">
            {addFiltered.map((item) => (
              <button
                key={item.typeId}
                type="button"
                onClick={() => {
                  const from = pending.from
                  const pos = pending.flow
                  setPending(null)
                  setAddQuery('')
                  if (onAddConnected) onAddConnected(item, pos, from)
                  else onAddAt(item, pos)
                }}
              >
                <span className={`palette__item-icon palette__item-icon--${visualCategoryForType(item.typeId)}`}>
                  <IconForType typeId={item.typeId} size={12} />
                </span>
                {t(item.labelKey)}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
