import type { Execution, NodeExecution } from '../types'
import { TYPE_IDS } from '../types'
import type { BoardNode } from '../canvas/BoardNode'
import { usePreferences } from '../settings/PreferencesContext'

interface Props {
  query: string
  onQueryChange: (value: string) => void
  busy: boolean
  onTest: () => void
  execution: Execution | null
  nodeExecutions: NodeExecution[]
  boardNodes: BoardNode[]
}

function outputOf(
  typeId: string,
  boardNodes: BoardNode[],
  execs: NodeExecution[],
): Record<string, unknown> | null {
  const ids = new Set(
    boardNodes.filter((n) => n.data.typeId === typeId).map((n) => n.id),
  )
  const hit = [...execs].reverse().find((n) => ids.has(n.node_id) && n.output)
  if (!hit || !hit.output || typeof hit.output !== 'object') return null
  return hit.output as Record<string, unknown>
}

export function QuerySandbox({
  query,
  onQueryChange,
  busy,
  onTest,
  execution,
  nodeExecutions,
  boardNodes,
}: Props) {
  const { t } = usePreferences()
  const classify = outputOf(TYPE_IDS.AI_CLASSIFY, boardNodes, nodeExecutions)
  const search = outputOf(TYPE_IDS.WEB_SEARCH, boardNodes, nodeExecutions)
  const answerChat = outputOf(TYPE_IDS.AI_CHAT, boardNodes, nodeExecutions)
  const answerTg = outputOf(
    TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE,
    boardNodes,
    nodeExecutions,
  )
  const answer =
    (typeof answerChat?.text === 'string' && answerChat.text) ||
    (typeof answerTg?.text === 'string' && answerTg.text) ||
    ''
  const results = Array.isArray(search?.results) ? search.results : []
  const sandbox =
    execution?.trigger &&
    typeof execution.trigger === 'object' &&
    execution.trigger !== null &&
    'sandbox' in execution.trigger &&
    (execution.trigger as { sandbox?: boolean }).sandbox === true
  const showResult = Boolean(classify || search || answer || (execution && sandbox))

  return (
    <section className="sandbox">
      <div className="sandbox__bar">
        <span className="sandbox__label">{t('sandbox.title')}</span>
        <textarea
          className="sandbox__input"
          rows={2}
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder={t('sandbox.placeholder')}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !busy) {
              e.preventDefault()
              onTest()
            }
          }}
        />
        <button
          type="button"
          className="btn btn--primary"
          disabled={busy || !query.trim()}
          onClick={onTest}
        >
          {busy ? t('sandbox.running') : t('sandbox.test')}
        </button>
      </div>
      <p className="sandbox__hint">{t('sandbox.hint')}</p>
      {showResult ? (
        <div className="sandbox__result">
          {classify && (
            <div>
              <h4>{t('sandbox.classify')}</h4>
              <p>
                {String(classify.label ?? '—')}
                {classify.needs_web_search ? ` · ${t('sandbox.willSearch')}` : ` · ${t('sandbox.noSearch')}`}
                {typeof classify.confidence === 'number'
                  ? ` · ${Math.round(classify.confidence * 100)}%`
                  : ''}
              </p>
              {typeof classify.query === 'string' && classify.query ? (
                <p className="sandbox__muted">{classify.query}</p>
              ) : null}
            </div>
          )}
          <div>
            <h4>{t('sandbox.search')}</h4>
            {results.length === 0 ? (
              <p className="sandbox__muted">{t('sandbox.noHits')}</p>
            ) : (
              <ul>
                {results.slice(0, 6).map((raw, i) => {
                  const hit = raw as {
                    title?: string
                    url?: string
                    snippet?: string
                  }
                  return (
                    <li key={`${hit.url ?? i}`}>
                      {hit.url ? (
                        <a href={hit.url} target="_blank" rel="noreferrer">
                          {hit.title || hit.url}
                        </a>
                      ) : (
                        (hit.title ?? '')
                      )}
                      {hit.snippet ? (
                        <span className="sandbox__muted"> — {hit.snippet}</span>
                      ) : null}
                    </li>
                  )
                })}
              </ul>
            )}
          </div>
          {answer ? (
            <div>
              <h4>{t('sandbox.answer')}</h4>
              <p className="sandbox__answer">{answer}</p>
              {answerTg && answerTg.sandbox ? (
                <p className="sandbox__muted">{t('sandbox.telegramSkipped')}</p>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}
