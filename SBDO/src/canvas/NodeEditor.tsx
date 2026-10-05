import { useEffect, useState } from 'react'
import type { BoardNodeData } from './BoardNode'
import { getConnections } from '../api/connections'
import type { Connection } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import { labelKeyForType } from './paletteItems'
import { NodeConfigForm } from './nodeConfigForms'
import type { MessageKey } from '../i18n/messages'
import { MOD } from '../ui/format'
import {
  IconClose,
  IconCopy,
  IconForType,
  IconTrash,
  visualCategoryForType,
} from '../ui/icons'
import { Tabs } from '../ui/primitives'

type InspectorTab = 'config' | 'advanced'

interface Props {
  nodeId: string
  data: BoardNodeData
  onClose: () => void
  onApply: (nodeId: string, config: Record<string, unknown>) => void
  onDelete: (nodeId: string) => void
  onDuplicate?: (nodeId: string) => void
}

export function NodeEditor({ nodeId, data, onClose, onApply, onDelete, onDuplicate }: Props) {
  const { t } = usePreferences()
  const [config, setConfig] = useState<Record<string, unknown>>({})
  const [connections, setConnections] = useState<Connection[]>([])
  const [tab, setTab] = useState<InspectorTab>('config')
  const [advancedText, setAdvancedText] = useState('{}')
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    const next = { ...(data.config ?? {}) }
    setConfig(next)
    setAdvancedText(JSON.stringify(next, null, 2))
    setError(null)
    setTab('config')
    // Only re-hydrate when switching nodes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodeId])

  useEffect(() => {
    void getConnections()
      .then(setConnections)
      .catch(() => setConnections([]))
  }, [nodeId])

  const labelKey = labelKeyForType(data.typeId)
  const label = labelKey ? t(labelKey) : data.label
  const category = visualCategoryForType(data.typeId)

  const updateConfig = (next: Record<string, unknown>) => {
    setConfig(next)
    setAdvancedText(JSON.stringify(next, null, 2))
    setError(null)
    onApply(nodeId, next)
  }

  return (
    <aside className="editor">
      <header className={`editor__header editor__header--${category}`}>
        <span className={`board-node__icon board-node__icon--${category} editor__icon`}>
          <IconForType typeId={data.typeId} size={16} />
        </span>
        <div className="editor__titles">
          <span className="editor__category">{t(`node.cat.${category}` as MessageKey)}</span>
          <h2>{label}</h2>
          <button
            type="button"
            className="editor__id"
            onClick={() => {
              void navigator.clipboard?.writeText(`{{nodes.${nodeId}.output}}`)
              setCopied(true)
              window.setTimeout(() => setCopied(false), 1200)
            }}
            data-tip={t('editor.copyRef')}
          >
            <code>{nodeId}</code>
            {copied ? <span className="editor__copied">✓</span> : <IconCopy size={11} />}
          </button>
        </div>
        <div className="editor__header-actions">
          {onDuplicate && (
            <button type="button" className="icon-btn" onClick={() => onDuplicate(nodeId)} data-tip={t('editor.duplicate')} aria-label={t('editor.duplicate')}>
              <IconCopy size={14} />
            </button>
          )}
          <button type="button" className="icon-btn" onClick={onClose} aria-label={t('editor.close')} data-tip={`${t('editor.close')} · Esc`}>
            <IconClose size={14} />
          </button>
        </div>
      </header>

      <div className="editor__tabs">
        <Tabs
          variant="pill"
          value={tab}
          onChange={setTab}
          items={[
            { id: 'config', label: t('editor.tab.config') },
            { id: 'advanced', label: t('editor.tab.advanced') },
          ]}
        />
      </div>

      <div className="editor__body">
        {tab === 'config' ? (
          <NodeConfigForm
            key={nodeId}
            typeId={data.typeId}
            config={config}
            onChange={updateConfig}
            connections={connections}
          />
        ) : (
          <label className="editor__field">
            <span>{t('editor.config')}</span>
            <span className="editor__hint">{t('editor.advancedHint')}</span>
            <textarea
              className="editor__json"
              value={advancedText}
              rows={18}
              spellCheck={false}
              onChange={(e) => {
                setAdvancedText(e.target.value)
                try {
                  const parsed = JSON.parse(e.target.value) as Record<string, unknown>
                  setConfig(parsed)
                  setError(null)
                  onApply(nodeId, parsed)
                } catch {
                  setError(t('editor.invalidJson'))
                }
              }}
            />
          </label>
        )}
        {error && <p className="editor__error">{error}</p>}
      </div>

      <div className="editor__actions">
        <span className="editor__autosave">{t('editor.autosave')}</span>
        <button type="button" className="btn btn--danger btn--sm" onClick={() => onDelete(nodeId)}>
          <IconTrash size={13} />
          {t('editor.delete')}
        </button>
      </div>
    </aside>
  )
}

interface WorkflowInspectorProps {
  name: string
  description: string
  version: number
  nodeCount: number
  edgeCount: number
  triggerCount: number
  onNameChange: (v: string) => void
  onDescriptionChange: (v: string) => void
  onAddTrigger: () => void
}

/** Inspector content when nothing is selected — workflow properties & shortcuts. */
export function WorkflowInspector({
  name,
  description,
  version,
  nodeCount,
  edgeCount,
  triggerCount,
  onNameChange,
  onDescriptionChange,
  onAddTrigger,
}: WorkflowInspectorProps) {
  const { t } = usePreferences()
  const shortcuts: [string, MessageKey][] = [
    ['R', 'shortcut.run'],
    ['T', 'shortcut.test'],
    [`${MOD}S`, 'shortcut.save'],
    [`${MOD}K`, 'shortcut.palette'],
    [`${MOD}Z`, 'shortcut.undo'],
    ['⌫', 'shortcut.delete'],
    ['Esc', 'shortcut.deselect'],
  ]
  return (
    <aside className="editor editor--workflow">
      <header className="editor__header">
        <div className="editor__titles">
          <span className="editor__category">{t('inspector.workflow')}</span>
          <h2>{name || t('top.untitled')}</h2>
          {version > 0 && <span className="editor__category">v{version}</span>}
        </div>
      </header>
      <div className="editor__body">
        <label className="editor__field">
          <span>{t('inspector.name')}</span>
          <input value={name} onChange={(e) => onNameChange(e.target.value)} />
        </label>
        <label className="editor__field">
          <span>{t('inspector.description')}</span>
          <textarea
            className="editor__plain"
            rows={3}
            value={description}
            placeholder={t('inspector.descriptionPlaceholder')}
            onChange={(e) => onDescriptionChange(e.target.value)}
          />
        </label>

        <div className="inspector-stats">
          <div>
            <strong>{nodeCount}</strong>
            <span>{t('inspector.nodes')}</span>
          </div>
          <div>
            <strong>{edgeCount}</strong>
            <span>{t('inspector.edges')}</span>
          </div>
          <div className={triggerCount === 0 ? 'is-warn' : ''}>
            <strong>{triggerCount}</strong>
            <span>{t('inspector.triggers')}</span>
          </div>
        </div>
        {triggerCount === 0 && (
          <div className="inspector-callout">
            <p>{t('inspector.noTrigger')}</p>
            <button type="button" className="btn btn--sm" onClick={onAddTrigger}>
              {t('inspector.addTrigger')}
            </button>
          </div>
        )}

        <p className="inspector-hint">{t('inspector.hint')}</p>

        <h3 className="editor__section-title">{t('inspector.shortcuts')}</h3>
        <ul className="shortcut-list">
          {shortcuts.map(([key, label]) => (
            <li key={label}>
              <span>{t(label)}</span>
              <kbd>{key}</kbd>
            </li>
          ))}
        </ul>
      </div>
    </aside>
  )
}
