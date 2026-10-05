import { useEffect, useState, type ReactNode } from 'react'
import {
  createChannel,
  createStream,
  createTrigger,
  deleteChannel,
  deleteStream,
  deleteTrigger,
  listChannels,
  listStreams,
  listTriggers,
  updateChannel,
  updateStream,
  updateTrigger,
} from '../api/ingress'
import type { Channel, ChannelKind, Stream, StreamDirection, Trigger, TriggerKind } from '../types'
import { usePreferences } from '../settings/PreferencesContext'
import { Modal, useConfirm } from '../ui/Modal'
import { Badge, Card, EmptyState, Field, Notice } from '../ui/primitives'
import {
  IconArrowRight,
  IconEdit,
  IconLightning,
  IconPlus,
  IconRadio,
  IconTrash,
  IconArrows,
} from '../ui/icons'

interface Props {
  open: boolean
  onClose: () => void
}

type Tab = 'channels' | 'streams' | 'triggers'

interface RowProps {
  icon: ReactNode
  title: string
  kind: string
  sub?: ReactNode
  disabled?: boolean
  editing?: boolean
  onEdit: () => void
  onDelete: () => void
}

function EntityRow({ icon, title, kind, sub, disabled, editing, onEdit, onDelete }: RowProps) {
  const { t } = usePreferences()
  return (
    <li className={`row${editing ? ' is-current' : ''}`}>
      <span className="row__icon">{icon}</span>
      <div className="row__main">
        <div className="row__title">
          <strong>{title}</strong>
          <span className="chip chip--mono">{kind}</span>
          {disabled && <Badge tone="warning">{t('ingress.disabled')}</Badge>}
        </div>
        {sub && <div className="row__meta">{sub}</div>}
      </div>
      <div className="row__actions">
        <button type="button" className="icon-btn" onClick={onEdit} aria-label={t('ingress.edit')} data-tip={t('ingress.edit')}>
          <IconEdit size={14} />
        </button>
        <button type="button" className="icon-btn icon-btn--danger" onClick={onDelete} aria-label={t('ingress.delete')} data-tip={t('ingress.delete')}>
          <IconTrash size={14} />
        </button>
      </div>
    </li>
  )
}

export function IngressPanel({ open, onClose }: Props) {
  const { t } = usePreferences()
  const confirm = useConfirm()
  const [tab, setTab] = useState<Tab>('channels')
  const [channels, setChannels] = useState<Channel[]>([])
  const [streams, setStreams] = useState<Stream[]>([])
  const [triggers, setTriggers] = useState<Trigger[]>([])
  const [message, setMessage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const [chName, setChName] = useState('')
  const [chKind, setChKind] = useState<ChannelKind>('webhook')
  const [chDesc, setChDesc] = useState('')
  const [editChannel, setEditChannel] = useState<Channel | null>(null)

  const [stChannelId, setStChannelId] = useState('')
  const [stName, setStName] = useState('')
  const [stDir, setStDir] = useState<StreamDirection>('inbound')
  const [editStream, setEditStream] = useState<Stream | null>(null)

  const [trStreamId, setTrStreamId] = useState('')
  const [trName, setTrName] = useState('')
  const [trKind, setTrKind] = useState<TriggerKind>('event')
  const [editTrigger, setEditTrigger] = useState<Trigger | null>(null)

  async function refresh() {
    try {
      const [chs, sts, trs] = await Promise.all([listChannels(), listStreams(), listTriggers()])
      setChannels(chs)
      setStreams(sts)
      setTriggers(trs)
      if (!stChannelId && chs[0]) setStChannelId(chs[0].id)
      if (!trStreamId && sts[0]) setTrStreamId(sts[0].id)
    } catch (err) {
      setMessage(String(err))
    }
  }

  useEffect(() => {
    if (open) {
      setMessage(null)
      void refresh()
    }
  }, [open])

  async function run(fn: () => Promise<void>, ok: string) {
    setBusy(true)
    setMessage(null)
    try {
      await fn()
      await refresh()
      setMessage(ok)
    } catch (err) {
      setMessage(String(err))
    } finally {
      setBusy(false)
    }
  }

  async function confirmDelete(text: string, fn: () => Promise<void>) {
    const ok = await confirm({ title: t('ingress.delete'), message: text, confirmLabel: t('ingress.delete'), danger: true })
    if (ok) await run(fn, t('ingress.deleted'))
  }

  function resetChannel() {
    setEditChannel(null)
    setChName('')
    setChDesc('')
  }
  function resetStream() {
    setEditStream(null)
    setStName('')
  }
  function resetTrigger() {
    setEditTrigger(null)
    setTrName('')
  }

  const steps: { id: Tab; label: string; count: number; icon: ReactNode; hint: string }[] = [
    { id: 'channels', label: t('ingress.tab.channels'), count: channels.length, icon: <IconRadio size={16} />, hint: t('ingress.step.channels') },
    { id: 'streams', label: t('ingress.tab.streams'), count: streams.length, icon: <IconArrows size={16} />, hint: t('ingress.step.streams') },
    { id: 'triggers', label: t('ingress.tab.triggers'), count: triggers.length, icon: <IconLightning size={16} />, hint: t('ingress.step.triggers') },
  ]

  const editing = tab === 'channels' ? editChannel : tab === 'streams' ? editStream : editTrigger
  const formTitle = editing
    ? t('ingress.editing', { name: editing.name })
    : tab === 'channels'
      ? t('ingress.newChannel')
      : tab === 'streams'
        ? t('ingress.newStream')
        : t('ingress.newTrigger')

  function cancelEdit() {
    if (tab === 'channels') resetChannel()
    else if (tab === 'streams') resetStream()
    else resetTrigger()
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      icon={<IconRadio size={18} />}
      title={t('ingress.title')}
      subtitle={t('ingress.hint')}
    >
      <div className="flow-steps">
        {steps.map((s, i) => (
          <div key={s.id} className="flow-steps__item">
            {i > 0 && <IconArrowRight size={16} className="flow-steps__arrow" />}
            <button type="button" className={`flow-step${tab === s.id ? ' is-active' : ''}`} onClick={() => setTab(s.id)}>
              <span className="flow-step__icon">{s.icon}</span>
              <span className="flow-step__text">
                <strong>
                  {s.label} <span className="flow-step__count">{s.count}</span>
                </strong>
                <span>{s.hint}</span>
              </span>
            </button>
          </div>
        ))}
      </div>

      <Notice text={message} onDismiss={() => setMessage(null)} />

      <div className="two-col">
        <Card
          title={formTitle}
          actions={
            editing && (
              <button type="button" className="btn btn--ghost btn--sm" onClick={cancelEdit}>
                {t('ingress.cancel')}
              </button>
            )
          }
        >
          {tab === 'channels' && (
            <form
              className="form"
              onSubmit={(e) => {
                e.preventDefault()
                void run(async () => {
                  if (editChannel) {
                    await updateChannel(editChannel.id, {
                      name: chName,
                      description: chDesc,
                      kind: chKind,
                      enabled: editChannel.enabled,
                      config: editChannel.config,
                    })
                  } else {
                    await createChannel({ name: chName, description: chDesc, kind: chKind, enabled: true, config: {} })
                  }
                  resetChannel()
                }, editChannel ? t('ingress.updated') : t('ingress.created'))
              }}
            >
              <Field label={t('ingress.name')}>
                <input value={chName} onChange={(e) => setChName(e.target.value)} required />
              </Field>
              <Field label={t('ingress.kind')}>
                <select value={chKind} onChange={(e) => setChKind(e.target.value as ChannelKind)}>
                  <option value="internal">internal</option>
                  <option value="webhook">webhook</option>
                  <option value="http">http</option>
                  <option value="telegram">telegram</option>
                </select>
              </Field>
              <Field label={t('ingress.description')}>
                <input value={chDesc} onChange={(e) => setChDesc(e.target.value)} />
              </Field>
              <button type="submit" className="btn btn--primary btn--block" disabled={busy}>
                {!editChannel && <IconPlus size={14} />}
                {editChannel ? t('ingress.save') : t('ingress.create')}
              </button>
            </form>
          )}

          {tab === 'streams' && (
            <form
              className="form"
              onSubmit={(e) => {
                e.preventDefault()
                void run(async () => {
                  if (editStream) {
                    await updateStream(editStream.id, {
                      channel_id: stChannelId,
                      name: stName,
                      description: editStream.description,
                      direction: stDir,
                      enabled: editStream.enabled,
                      config: editStream.config,
                    })
                  } else {
                    await createStream({ channel_id: stChannelId, name: stName, direction: stDir, enabled: true, config: {} })
                  }
                  resetStream()
                }, editStream ? t('ingress.updated') : t('ingress.created'))
              }}
            >
              {channels.length === 0 && <Notice tone="warning" text={t('ingress.needChannel')} />}
              <Field label={t('ingress.channel')}>
                <select value={stChannelId} onChange={(e) => setStChannelId(e.target.value)} required>
                  <option value="">{t('ingress.selectChannel')}</option>
                  {channels.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={t('ingress.name')}>
                <input value={stName} onChange={(e) => setStName(e.target.value)} required />
              </Field>
              <Field label={t('ingress.direction')}>
                <select value={stDir} onChange={(e) => setStDir(e.target.value as StreamDirection)}>
                  <option value="inbound">inbound</option>
                  <option value="outbound">outbound</option>
                </select>
              </Field>
              <button type="submit" className="btn btn--primary btn--block" disabled={busy || !stChannelId}>
                {!editStream && <IconPlus size={14} />}
                {editStream ? t('ingress.save') : t('ingress.create')}
              </button>
            </form>
          )}

          {tab === 'triggers' && (
            <form
              className="form"
              onSubmit={(e) => {
                e.preventDefault()
                void run(async () => {
                  if (editTrigger) {
                    await updateTrigger(editTrigger.id, {
                      stream_id: trStreamId,
                      name: trName,
                      description: editTrigger.description,
                      kind: trKind,
                      enabled: editTrigger.enabled,
                      workflow_id: editTrigger.workflow_id,
                      config: editTrigger.config,
                    })
                  } else {
                    await createTrigger({ stream_id: trStreamId, name: trName, kind: trKind, enabled: true, config: {} })
                  }
                  resetTrigger()
                }, editTrigger ? t('ingress.updated') : t('ingress.created'))
              }}
            >
              {streams.length === 0 && <Notice tone="warning" text={t('ingress.needStream')} />}
              <Field label={t('ingress.stream')}>
                <select value={trStreamId} onChange={(e) => setTrStreamId(e.target.value)} required>
                  <option value="">{t('ingress.selectStream')}</option>
                  {streams.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={t('ingress.name')}>
                <input value={trName} onChange={(e) => setTrName(e.target.value)} required />
              </Field>
              <Field label={t('ingress.kind')}>
                <select value={trKind} onChange={(e) => setTrKind(e.target.value as TriggerKind)}>
                  <option value="event">event</option>
                  <option value="webhook">webhook</option>
                  <option value="schedule">schedule</option>
                  <option value="manual">manual</option>
                </select>
              </Field>
              <button type="submit" className="btn btn--primary btn--block" disabled={busy || !trStreamId}>
                {!editTrigger && <IconPlus size={14} />}
                {editTrigger ? t('ingress.save') : t('ingress.create')}
              </button>
            </form>
          )}
        </Card>

        <div className="two-col__list">
          {tab === 'channels' &&
            (channels.length === 0 ? (
              <EmptyState compact icon={<IconRadio size={20} />} title={t('ingress.emptyChannels')} hint={t('ingress.step.channels')} />
            ) : (
              <ul className="row-list">
                {channels.map((c) => (
                  <EntityRow
                    key={c.id}
                    icon={<IconRadio size={15} />}
                    title={c.name}
                    kind={c.kind}
                    disabled={!c.enabled}
                    editing={editChannel?.id === c.id}
                    sub={
                      <>
                        {c.description && <span>{c.description}</span>}
                        <span>{t('ingress.streamsCount', { count: streams.filter((s) => s.channel_id === c.id).length })}</span>
                      </>
                    }
                    onEdit={() => {
                      setEditChannel(c)
                      setChName(c.name)
                      setChKind(c.kind)
                      setChDesc(c.description)
                    }}
                    onDelete={() => void confirmDelete(t('ingress.confirmDeleteChannel', { name: c.name }), () => deleteChannel(c.id))}
                  />
                ))}
              </ul>
            ))}

          {tab === 'streams' &&
            (streams.length === 0 ? (
              <EmptyState compact icon={<IconArrows size={20} />} title={t('ingress.emptyStreams')} hint={t('ingress.step.streams')} />
            ) : (
              <ul className="row-list">
                {streams.map((s) => (
                  <EntityRow
                    key={s.id}
                    icon={<IconArrows size={15} />}
                    title={s.name}
                    kind={s.direction}
                    disabled={!s.enabled}
                    editing={editStream?.id === s.id}
                    sub={
                      <>
                        <span>
                          <IconRadio size={12} />
                          {channels.find((c) => c.id === s.channel_id)?.name ?? s.channel_id.slice(0, 8)}
                        </span>
                        <span>{t('ingress.triggersCount', { count: triggers.filter((tr) => tr.stream_id === s.id).length })}</span>
                      </>
                    }
                    onEdit={() => {
                      setEditStream(s)
                      setStName(s.name)
                      setStChannelId(s.channel_id)
                      setStDir(s.direction)
                    }}
                    onDelete={() => void confirmDelete(t('ingress.confirmDeleteStream', { name: s.name }), () => deleteStream(s.id))}
                  />
                ))}
              </ul>
            ))}

          {tab === 'triggers' &&
            (triggers.length === 0 ? (
              <EmptyState compact icon={<IconLightning size={20} />} title={t('ingress.emptyTriggers')} hint={t('ingress.step.triggers')} />
            ) : (
              <ul className="row-list">
                {triggers.map((tr) => (
                  <EntityRow
                    key={tr.id}
                    icon={<IconLightning size={15} />}
                    title={tr.name}
                    kind={tr.kind}
                    disabled={!tr.enabled}
                    editing={editTrigger?.id === tr.id}
                    sub={
                      <span>
                        <IconArrows size={12} />
                        {streams.find((s) => s.id === tr.stream_id)?.name ?? tr.stream_id.slice(0, 8)}
                      </span>
                    }
                    onEdit={() => {
                      setEditTrigger(tr)
                      setTrName(tr.name)
                      setTrStreamId(tr.stream_id)
                      setTrKind(tr.kind)
                    }}
                    onDelete={() => void confirmDelete(t('ingress.confirmDeleteTrigger', { name: tr.name }), () => deleteTrigger(tr.id))}
                  />
                ))}
              </ul>
            ))}
        </div>
      </div>
    </Modal>
  )
}
