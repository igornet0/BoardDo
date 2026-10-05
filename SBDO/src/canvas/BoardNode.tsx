import { Handle, Position, type NodeProps, type Node } from '@xyflow/react'
import { TYPE_IDS } from '../types'
import { labelKeyForType } from './paletteItems'
import { usePreferences } from '../settings/PreferencesContext'
import type { MessageKey } from '../i18n/messages'
import {
  IconAlert,
  IconCheck,
  IconForType,
  visualCategoryForType,
  type NodeVisualCategory,
} from '../ui/icons'

export type BoardNodeData = {
  typeId: string
  label: string
  config: Record<string, unknown>
  runStatus?: 'idle' | 'running' | 'completed' | 'failed'
  durationMs?: number
}

export type BoardNode = Node<BoardNodeData, 'board'>

const CATEGORY_KEY: Record<NodeVisualCategory, MessageKey> = {
  trigger: 'node.cat.trigger',
  logic: 'node.cat.logic',
  data: 'node.cat.data',
  ai: 'node.cat.ai',
  action: 'node.cat.action',
  integration: 'node.cat.integration',
}

const SUMMARY_KEYS = ['message', 'text', 'prompt', 'url', 'expression', 'cron', 'query', 'caption', 'name']

/** One-line preview of the most meaningful config value. */
export function configSummary(config: Record<string, unknown> | undefined): string | null {
  if (!config) return null
  for (const key of SUMMARY_KEYS) {
    const v = config[key]
    if (typeof v === 'string' && v.trim()) return v.trim()
  }
  const every = config.every
  if (every && typeof every === 'object') {
    const parts = Object.entries(every as Record<string, unknown>).map(([k, v]) => `${v} ${k}`)
    if (parts.length) return `every ${parts.join(' ')}`
  }
  if (typeof config.method === 'string') return config.method
  return null
}

export function BoardNodeView({ id, data, selected }: NodeProps<BoardNode>) {
  const { t } = usePreferences()
  const isCondition = data.typeId === TYPE_IDS.LOGIC_CONDITION
  const isTrigger = data.typeId.startsWith('trigger.')
  const status = data.runStatus ?? 'idle'
  const labelKey = labelKeyForType(data.typeId)
  const label = labelKey ? t(labelKey) : data.label
  const category = visualCategoryForType(data.typeId)
  const summary = configSummary(data.config)

  return (
    <div
      className={`board-node board-node--${category} board-node--${status}${selected ? ' board-node--selected' : ''}`}
    >
      {!isTrigger && <Handle type="target" position={Position.Top} className="board-handle" />}

      <div className="board-node__head">
        <span className={`board-node__icon board-node__icon--${category}`}>
          <IconForType typeId={data.typeId} size={14} />
        </span>
        <div className="board-node__titles">
          <span className="board-node__category">{t(CATEGORY_KEY[category])}</span>
          <span className="board-node__label">{label}</span>
        </div>
        {status !== 'idle' && (
          <span className={`board-node__status board-node__status--${status}`} title={t(`node.status.${status}` as MessageKey)}>
            {status === 'completed' ? (
              <IconCheck size={11} />
            ) : status === 'failed' ? (
              <IconAlert size={11} />
            ) : (
              <span className="spinner spinner--xs" />
            )}
          </span>
        )}
      </div>

      {summary && <div className="board-node__summary">{summary}</div>}

      <div className="board-node__foot">
        <code>{id}</code>
        {status === 'completed' && data.durationMs != null && <span>{data.durationMs} ms</span>}
        {status === 'failed' && <span className="board-node__fail">{t('node.status.failed')}</span>}
      </div>

      {isCondition ? (
        <>
          <div className="board-node__ports">
            <span className="board-node__port board-node__port--true">true</span>
            <span className="board-node__port board-node__port--false">false</span>
          </div>
          <Handle type="source" position={Position.Bottom} id="true" style={{ left: '28%' }} className="board-handle board-handle--true" />
          <Handle type="source" position={Position.Bottom} id="false" style={{ left: '72%' }} className="board-handle board-handle--false" />
        </>
      ) : (
        <Handle type="source" position={Position.Bottom} className="board-handle" />
      )}
    </div>
  )
}
