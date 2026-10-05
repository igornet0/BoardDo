import { useCallback, useEffect, useState } from 'react'

import { createTool, listTools, runTool, type ToolSummary } from '../api/tools'
import { usePreferences } from '../settings/PreferencesContext'
import { Modal } from '../ui/Modal'
import { Badge, Card, EmptyState, Notice } from '../ui/primitives'
import { IconPlay, IconPlus, IconRefresh, IconWrench } from '../ui/icons'

type Props = {
  open: boolean
  onClose: () => void
}

export function ToolsPanel({ open, onClose }: Props) {
  const { t } = usePreferences()
  const [tools, setTools] = useState<ToolSummary[]>([])
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [output, setOutput] = useState<{ tool: string; text: string } | null>(null)
  const [prompt, setPrompt] = useState('')
  const [selected, setSelected] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const res = await listTools()
      setTools(res.tools)
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (open) {
      setMessage(null)
      void refresh()
    }
  }, [open, refresh])

  async function handleCreateFromPrompt() {
    if (!prompt.trim()) return
    setBusy(true)
    setMessage(null)
    try {
      const name = 'user_tool_' + Date.now().toString(36)
      const code = `# ${prompt}\ndef run(input_data):\n    return {"echo": input_data}\n`
      await createTool({
        name,
        description: prompt,
        code,
        test_input: {},
        output_schema: { type: 'object', required: ['echo'] },
      })
      setMessage(t('tools.created'))
      setPrompt('')
      await refresh()
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  async function handleTestRun(tool: ToolSummary) {
    setBusy(true)
    setSelected(tool.id)
    setMessage(null)
    try {
      const out = await runTool(tool.name, { numbers: [1, 2, 3, 4, 5] })
      setOutput({ tool: tool.name, text: JSON.stringify(out, null, 2) })
      await refresh()
    } catch (e) {
      setOutput({ tool: tool.name, text: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      icon={<IconWrench size={18} />}
      title={t('tools.title')}
      subtitle={t('tools.hint')}
      actions={
        <button type="button" className="icon-btn" onClick={() => void refresh()} aria-label={t('common.refresh')} data-tip={t('common.refresh')}>
          <IconRefresh size={15} className={loading ? 'spin' : undefined} />
        </button>
      }
    >
      <Card title={t('tools.create')} subtitle={t('tools.createHint')}>
        <div className="composer">
          <textarea
            rows={3}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder={t('tools.createPlaceholder')}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void handleCreateFromPrompt()
            }}
          />
          <button type="button" className="btn btn--primary" disabled={busy || !prompt.trim()} onClick={() => void handleCreateFromPrompt()}>
            <IconPlus size={14} />
            {t('tools.createButton')}
          </button>
        </div>
      </Card>

      <Notice text={message} onDismiss={() => setMessage(null)} />

      <h3 className="section-title">
        {t('tools.list')} <span className="section-title__count">{tools.length}</span>
      </h3>
      {tools.length === 0 ? (
        <EmptyState compact icon={<IconWrench size={20} />} title={t('tools.empty')} hint={t('tools.emptyHint')} />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>{t('tools.colName')}</th>
                <th>{t('tools.colStatus')}</th>
                <th className="num">{t('tools.colSuccess')}</th>
                <th className="num">{t('tools.colAvgMs')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {tools.map((tool) => {
                const stats = tool.execution_stats
                const rate = stats.execution_count > 0 ? Math.round((100 * stats.success_count) / stats.execution_count) : null
                return (
                  <tr key={tool.id} className={selected === tool.id ? 'is-selected' : ''}>
                    <td>
                      <strong className="mono">{tool.name}</strong>
                      <div className="table__sub">{tool.description}</div>
                    </td>
                    <td>
                      {tool.active_version_id ? <Badge tone="success" dot>{t('tools.active')}</Badge> : <Badge>—</Badge>}
                    </td>
                    <td className="num">
                      {rate == null ? '—' : <span className={rate >= 90 ? 'tone-success' : rate >= 50 ? 'tone-warning' : 'tone-error'}>{rate}%</span>}
                    </td>
                    <td className="num mono">{stats.average_duration_ms || '—'}</td>
                    <td className="num">
                      <button type="button" className="btn btn--sm" disabled={busy} onClick={() => void handleTestRun(tool)}>
                        <IconPlay size={11} />
                        {t('tools.run')}
                      </button>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}

      {output && (
        <div className="code-block">
          <div className="code-block__head">
            <span>
              {t('tools.output')} · <code>{output.tool}</code>
            </span>
            <button type="button" className="btn btn--ghost btn--xs" onClick={() => setOutput(null)}>
              {t('common.close')}
            </button>
          </div>
          <pre>{output.text}</pre>
        </div>
      )}
    </Modal>
  )
}
