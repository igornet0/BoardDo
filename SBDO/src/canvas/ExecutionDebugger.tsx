import type { Execution, NodeExecution } from '../types'
import type { BoardNode } from './BoardNode'
import { labelKeyForType } from './paletteItems'
import { usePreferences } from '../settings/PreferencesContext'
import { IconActivity, IconAlert, IconArrowRight, IconCheck, IconDot, IconForType } from '../ui/icons'
import { Badge, EmptyState } from '../ui/primitives'

interface Props {
  execution: Execution | null
  nodes: NodeExecution[]
  boardNodes: BoardNode[]
  selectedNodeId: string | null
  onSelectNode: (nodeId: string) => void
  onRevealNode?: (nodeId: string) => void
}

function statusTone(status: string) {
  if (status === 'completed') return 'success' as const
  if (status === 'failed') return 'error' as const
  if (status === 'running' || status === 'pending') return 'warning' as const
  return 'neutral' as const
}

export function ExecutionDebugger({
  execution,
  nodes,
  boardNodes,
  selectedNodeId,
  onSelectNode,
  onRevealNode,
}: Props) {
  const { t } = usePreferences()
  const selected =
    nodes.find((n) => n.node_id === selectedNodeId) ??
    nodes.find((n) => n.status === 'failed') ??
    null
  const totalMs = nodes.reduce((sum, n) => sum + (n.duration_ms ?? 0), 0)
  const maxMs = Math.max(1, ...nodes.map((n) => n.duration_ms ?? 0))

  function typeOf(nodeId: string) {
    return boardNodes.find((b) => b.id === nodeId)?.data.typeId ?? ''
  }
  function labelOf(nodeId: string) {
    const key = labelKeyForType(typeOf(nodeId))
    return key ? t(key) : nodeId
  }

  if (!execution && nodes.length === 0) {
    return (
      <EmptyState
        compact
        icon={<IconActivity size={18} />}
        title={t('debugger.idle')}
        hint={t('debugger.idleHint')}
      />
    )
  }

  return (
    <div className="debugger">
      <div className="debugger__list">
        {execution && (
          <div className="debugger__meta">
            <Badge tone={statusTone(execution.status)} dot>
              {execution.status}
            </Badge>
            <code>#{execution.id.slice(0, 8)}</code>
            <span className="debugger__total">{totalMs} ms</span>
          </div>
        )}
        {execution?.error && <div className="debugger__error">{execution.error}</div>}
        <ul>
          {nodes.map((n) => {
            const active = selected?.node_id === n.node_id
            return (
              <li key={n.id}>
                <button
                  type="button"
                  className={active ? 'is-active' : ''}
                  onClick={() => onSelectNode(n.node_id)}
                >
                  <span className={`debugger__mark debugger__mark--${n.status}`}>
                    {n.status === 'completed' ? (
                      <IconCheck size={12} />
                    ) : n.status === 'failed' ? (
                      <IconAlert size={12} />
                    ) : (
                      <IconDot size={12} />
                    )}
                  </span>
                  <span className="debugger__name">
                    <IconForType typeId={typeOf(n.node_id)} size={12} />
                    <span>{labelOf(n.node_id)}</span>
                    <code>{n.node_id}</code>
                  </span>
                  <span className="debugger__bar" aria-hidden>
                    <i style={{ width: `${((n.duration_ms ?? 0) / maxMs) * 100}%` }} />
                  </span>
                  <span className="debugger__ms">{n.duration_ms ?? 0} ms</span>
                </button>
              </li>
            )
          })}
        </ul>
      </div>
      <div className="debugger__detail">
        {selected ? (
          <>
            <div className="debugger__detail-head">
              <h3>{labelOf(selected.node_id)}</h3>
              <code>{selected.node_id}</code>
              {onRevealNode && (
                <button type="button" className="btn btn--ghost btn--xs" onClick={() => onRevealNode(selected.node_id)}>
                  {t('debugger.reveal')}
                  <IconArrowRight size={12} />
                </button>
              )}
            </div>
            <div className="debugger__io">
              <div>
                <h4>{t('debugger.input')}</h4>
                <pre>{JSON.stringify(selected.input, null, 2)}</pre>
              </div>
              <div>
                <h4>{selected.error ? t('debugger.error') : t('debugger.output')}</h4>
                <pre className={selected.error ? 'is-error' : ''}>
                  {selected.error ? selected.error : JSON.stringify(selected.output ?? null, null, 2)}
                </pre>
              </div>
            </div>
          </>
        ) : (
          <p className="debugger__idle">{t('debugger.selectNode')}</p>
        )}
      </div>
    </div>
  )
}
