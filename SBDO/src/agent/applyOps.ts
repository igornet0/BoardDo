import type { Edge } from '@xyflow/react'
import type { AgentGraphOp } from '../api/agent'
import type { BoardNode } from '../canvas/BoardNode'
import {
  createBoardNode,
  nextId,
} from '../canvas/WorkflowCanvas'
import { PALETTE as PALETTE_ITEMS } from '../canvas/paletteItems'

export interface GraphMeta {
  name: string
  description: string
}

export interface AppliedGraph {
  nodes: BoardNode[]
  edges: Edge[]
  meta: GraphMeta
}

function mergeConfig(
  base: Record<string, unknown>,
  incoming: Record<string, unknown> | undefined,
): Record<string, unknown> {
  if (!incoming) return { ...base }
  return { ...base, ...incoming }
}

function paletteFor(typeId: string) {
  return PALETTE_ITEMS.find((p) => p.typeId === typeId) ?? null
}

export function applyOps(
  ops: AgentGraphOp[],
  nodes: BoardNode[],
  edges: Edge[],
  meta: GraphMeta,
): AppliedGraph {
  let nextNodes = [...nodes]
  let nextEdges = [...edges]
  let nextMeta = { ...meta }
  let yCursor =
    nextNodes.reduce((max, n) => Math.max(max, n.position.y), 40) + 140

  for (const op of ops) {
    switch (op.op) {
      case 'add_node': {
        const item = paletteFor(op.type_id)
        if (!item) break
        const position = op.position ?? { x: 280, y: yCursor }
        yCursor = position.y + 140
        const node = createBoardNode(item, position)
        if (op.id) node.id = op.id
        node.data.config = mergeConfig(
          structuredClone(item.defaultConfig),
          op.config ?? undefined,
        )
        nextNodes = [...nextNodes, node]
        break
      }
      case 'update_node': {
        nextNodes = nextNodes.map((n) => {
          if (n.id !== op.id) return n
          return {
            ...n,
            position: op.position ?? n.position,
            data: {
              ...n.data,
              config: mergeConfig(
                n.data.config,
                (op.config as Record<string, unknown> | null) ?? undefined,
              ),
            },
          }
        })
        break
      }
      case 'remove_node': {
        nextNodes = nextNodes.filter((n) => n.id !== op.id)
        nextEdges = nextEdges.filter(
          (e) => e.source !== op.id && e.target !== op.id,
        )
        break
      }
      case 'add_edge': {
        nextEdges = [
          ...nextEdges,
          {
            id: op.id || nextId('e'),
            source: op.source,
            target: op.target,
            sourceHandle: op.source_port ?? undefined,
            targetHandle: op.target_port ?? undefined,
          },
        ]
        break
      }
      case 'remove_edge': {
        nextEdges = nextEdges.filter((e) => {
          if (op.id) return e.id !== op.id
          if (op.source && op.target) {
            return !(e.source === op.source && e.target === op.target)
          }
          return true
        })
        break
      }
      case 'set_meta': {
        if (op.name) nextMeta.name = op.name
        if (op.description) nextMeta.description = op.description
        break
      }
      default:
        break
    }
  }

  return { nodes: nextNodes, edges: nextEdges, meta: nextMeta }
}
