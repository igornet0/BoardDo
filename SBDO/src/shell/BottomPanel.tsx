import type { ReactNode } from 'react'
import type { Execution, NodeExecution } from '../types'
import type { BoardNode } from '../canvas/BoardNode'
import { QuerySandbox } from '../components/QuerySandbox'
import { ExecutionDebugger } from '../canvas/ExecutionDebugger'
import { usePreferences } from '../settings/PreferencesContext'
import {
  IconActivity,
  IconChevron,
  IconFlask,
  IconTerminal,
  IconTrash,
  IconVariable,
} from '../ui/icons'

export type BottomTab = 'console' | 'execution' | 'test' | 'variables'

interface Props {
  open: boolean
  tab: BottomTab
  onTabChange: (tab: BottomTab) => void
  onToggle: () => void
  askText: string
  onAskTextChange: (v: string) => void
  busy: boolean
  onTest: () => void
  execution: Execution | null
  nodeExecutions: NodeExecution[]
  boardNodes: BoardNode[]
  edgeCount: number
  debugNodeId: string | null
  onSelectDebugNode: (id: string) => void
  onSelectCanvasNode: (id: string) => void
  consoleLines: string[]
  onClearConsole: () => void
}

function lineTone(line: string): string {
  if (line.includes('failed')) return 'console__line--error'
  if (line.includes('completed')) return 'console__line--ok'
  if (line.trimStart().startsWith('log')) return 'console__line--log'
  return ''
}

export function BottomPanel({
  open,
  tab,
  onTabChange,
  onToggle,
  askText,
  onAskTextChange,
  busy,
  onTest,
  execution,
  nodeExecutions,
  boardNodes,
  edgeCount,
  debugNodeId,
  onSelectDebugNode,
  onSelectCanvasNode,
  consoleLines,
  onClearConsole,
}: Props) {
  const { t } = usePreferences()

  const execTone =
    execution?.status === 'failed'
      ? 'error'
      : execution?.status === 'completed'
        ? 'success'
        : execution
          ? 'warning'
          : null

  const tabs: { id: BottomTab; label: string; icon: ReactNode; extra?: ReactNode }[] = [
    {
      id: 'console',
      label: t('bottom.console'),
      icon: <IconTerminal size={13} />,
      extra: consoleLines.length > 0 ? <span className="bottom-tab__count">{consoleLines.length}</span> : null,
    },
    {
      id: 'execution',
      label: t('bottom.execution'),
      icon: <IconActivity size={13} />,
      extra: execTone ? <span className={`bottom-tab__dot bottom-tab__dot--${execTone}`} /> : null,
    },
    { id: 'test', label: t('bottom.test'), icon: <IconFlask size={13} /> },
    { id: 'variables', label: t('bottom.variables'), icon: <IconVariable size={13} /> },
  ]

  const triggers = boardNodes.filter((n) => n.data.typeId.startsWith('trigger.')).length

  return (
    <section className={`bottom-panel${open ? ' is-open' : ''}`}>
      <div className="bottom-panel__bar">
        <div className="bottom-panel__tabs" role="tablist">
          {tabs.map((item) => {
            const active = open && tab === item.id
            return (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={active}
                className={`bottom-tab${active ? ' is-active' : ''}`}
                onClick={() => {
                  if (active) onToggle()
                  else {
                    onTabChange(item.id)
                    if (!open) onToggle()
                  }
                }}
              >
                {item.icon}
                <span>{item.label}</span>
                {item.extra}
              </button>
            )
          })}
        </div>

        <div className="bottom-panel__status">
          {open && tab === 'console' && consoleLines.length > 0 && (
            <button type="button" className="btn btn--ghost btn--xs" onClick={onClearConsole}>
              <IconTrash size={12} />
              {t('bottom.clear')}
            </button>
          )}
          <span className="status-item">{t('bottom.nodes', { count: boardNodes.length })}</span>
          <span className="status-item">{t('bottom.edges', { count: edgeCount })}</span>
          <span className="status-item">{t('bottom.triggers', { count: triggers })}</span>
          <button
            type="button"
            className="icon-btn icon-btn--sm"
            onClick={onToggle}
            aria-expanded={open}
            aria-label={open ? t('bottom.collapse') : t('bottom.expand')}
          >
            <IconChevron size={14} style={{ transform: open ? undefined : 'rotate(180deg)' }} />
          </button>
        </div>
      </div>

      {open && (
        <div className="bottom-panel__body">
          {tab === 'console' && (
            <div className="console">
              {consoleLines.length === 0 ? (
                <p className="console__empty">{t('bottom.consoleEmpty')}</p>
              ) : (
                consoleLines.map((line, i) => (
                  <div key={i} className={`console__line ${lineTone(line)}`}>
                    <span className="console__no">{i + 1}</span>
                    <span>{line}</span>
                  </div>
                ))
              )}
            </div>
          )}

          {tab === 'execution' && (
            <ExecutionDebugger
              execution={execution}
              nodes={nodeExecutions}
              boardNodes={boardNodes}
              selectedNodeId={debugNodeId}
              onSelectNode={onSelectDebugNode}
              onRevealNode={onSelectCanvasNode}
            />
          )}

          {tab === 'test' && (
            <QuerySandbox
              query={askText}
              onQueryChange={onAskTextChange}
              busy={busy}
              onTest={onTest}
              execution={execution}
              nodeExecutions={nodeExecutions}
              boardNodes={boardNodes}
            />
          )}

          {tab === 'variables' && (
            <div className="vars">
              <p className="vars__hint">{t('bottom.variablesHint')}</p>
              <ul className="vars__list">
                <li>
                  <code>{'{{trigger.*}}'}</code>
                  <span>{t('bottom.var.trigger')}</span>
                </li>
                <li>
                  <code>{'{{nodes.<id>.output.*}}'}</code>
                  <span>{t('bottom.var.nodes')}</span>
                </li>
                <li>
                  <code>{'{{variables.*}}'}</code>
                  <span>{t('bottom.var.vars')}</span>
                </li>
                <li>
                  <code>{'{{amount > 100}}'}</code>
                  <span>{t('bottom.var.expr')}</span>
                </li>
              </ul>
              {boardNodes.length > 0 && (
                <>
                  <p className="vars__hint">{t('bottom.var.available')}</p>
                  <div className="vars__chips">
                    {boardNodes.map((n) => (
                      <button key={n.id} type="button" className="chip chip--mono chip--btn" onClick={() => onSelectCanvasNode(n.id)}>
                        {`nodes.${n.id}`}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  )
}
