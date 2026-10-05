import type { RuntimeSnapshot, RuntimeState } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import {
  IconFlask,
  IconPlay,
  IconRedo,
  IconSave,
  IconSearch,
  IconStop,
  IconUndo,
} from '../ui/icons'
import logoUrl from '../assets/logo.png'
import { MOD } from '../ui/format'

interface Props {
  workflowName: string
  onWorkflowNameChange: (name: string) => void
  version: number
  dirty: boolean
  busy: boolean
  canUndo: boolean
  canRedo: boolean
  runtime: RuntimeSnapshot | null
  liveRuntimes: number
  onSave: () => void
  onRun: () => void
  onTest: () => void
  onUndo: () => void
  onRedo: () => void
  onOpenCommand: () => void
  onOpenProjects: () => void
  onOpenRuntime: () => void
  onStartRuntime: () => void
  onStopRuntime: () => void
}

function runtimeDotClass(state: RuntimeState | undefined): string {
  if (!state || state === 'stopped') return 'runtime-dot runtime-dot--stopped'
  if (state === 'executing') return 'runtime-dot runtime-dot--executing'
  if (state === 'starting' || state === 'stopping') return 'runtime-dot runtime-dot--starting'
  return 'runtime-dot runtime-dot--waiting'
}


export function TopBar({
  workflowName,
  onWorkflowNameChange,
  version,
  dirty,
  busy,
  canUndo,
  canRedo,
  runtime,
  liveRuntimes,
  onSave,
  onRun,
  onTest,
  onUndo,
  onRedo,
  onOpenCommand,
  onOpenProjects,
  onOpenRuntime,
  onStartRuntime,
  onStopRuntime,
}: Props) {
  const { t } = usePreferences()
  const armed = runtime != null && runtime.state !== 'stopped' && runtime.state !== 'stopping'
  const stateLabel = runtime
    ? t(`runtime.state.${runtime.state}` as 'runtime.state.running')
    : t('runtime.state.stopped')

  return (
    <header className="topbar">
      <div className="topbar__left">
        <div className="brand" title="BoardDo">
          <img className="brand__logo" src={logoUrl} alt="" width={24} height={24} decoding="async" />
          <span className="brand__mark">BoardDo</span>
        </div>

        <nav className="crumbs" aria-label="breadcrumb">
          <button type="button" className="crumbs__link" onClick={onOpenProjects}>
            {t('nav.projects')}
          </button>
          <span className="crumbs__sep">/</span>
          <input
            className="crumbs__name"
            value={workflowName}
            placeholder={t('top.untitled')}
            onChange={(e) => onWorkflowNameChange(e.target.value)}
            aria-label={t('app.workflowName')}
            size={Math.max(8, Math.min(28, workflowName.length + 1))}
          />
          {version > 0 && <span className="chip chip--mono">v{version}</span>}
          <span
            className={`save-state${dirty ? ' save-state--dirty' : ''}`}
            title={dirty ? t('app.unsaved') : t('app.savedHint')}
          >
            <span className="save-state__dot" />
            <span className="save-state__label">{dirty ? t('top.unsaved') : t('top.saved')}</span>
          </span>
        </nav>
      </div>

      <button type="button" className="topbar__search" onClick={onOpenCommand}>
        <IconSearch size={14} />
        <span>{t('top.search')}</span>
        <kbd>{MOD}K</kbd>
      </button>

      <div className="topbar__right">
        <div className="btn-group">
          <button
            type="button"
            className="icon-btn"
            disabled={!canUndo}
            onClick={onUndo}
            data-tip={`${t('cmd.undo')} · ${MOD}Z`}
            aria-label={t('cmd.undo')}
          >
            <IconUndo size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            disabled={!canRedo}
            onClick={onRedo}
            data-tip={`${t('cmd.redo')} · ${MOD}⇧Z`}
            aria-label={t('cmd.redo')}
          >
            <IconRedo size={15} />
          </button>
        </div>

        <div className={`runtime-control${armed ? ' is-armed' : ''}`}>
          <button type="button" className="runtime-control__chip" onClick={onOpenRuntime} data-tip={t('runtime.panel.title')}>
            <span className={runtimeDotClass(runtime?.state)} aria-hidden />
            <span className="runtime-control__label">{stateLabel}</span>
            {liveRuntimes > 0 && <span className="runtime-control__count">{liveRuntimes}</span>}
          </button>
          <button
            type="button"
            className="runtime-control__toggle"
            disabled={busy}
            onClick={armed ? onStopRuntime : onStartRuntime}
            data-tip={armed ? t('runtime.stop') : t('runtime.startHint')}
            aria-label={armed ? t('runtime.stop') : t('runtime.start')}
          >
            {armed ? <IconStop size={12} /> : <IconPlay size={12} />}
          </button>
        </div>

        <span className="topbar__divider" aria-hidden />

        <button
          type="button"
          className="icon-btn"
          disabled={busy}
          onClick={onSave}
          data-tip={`${t('app.save')} · ${MOD}S`}
          aria-label={t('app.save')}
        >
          <IconSave size={15} />
        </button>
        <button type="button" className="btn topbar__test" disabled={busy} onClick={onTest} data-tip={`${t('sandbox.test')} · T`}>
          <IconFlask size={14} />
          <span>{t('sandbox.test')}</span>
        </button>
        <button type="button" className="btn btn--primary topbar__run" disabled={busy} onClick={onRun} data-tip={`${t('app.run')} · R`}>
          {busy ? <span className="spinner" aria-hidden /> : <IconPlay size={13} />}
          <span>{t('app.run')}</span>
        </button>
      </div>
    </header>
  )
}
