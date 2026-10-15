import { useCallback, useEffect, useMemo, useState, type FormEvent } from 'react'
import {
  changeDealStage,
  createDeal,
  createProduct,
  dealFromLead,
  deleteDeal,
  deleteProduct,
  getDeal,
  getSalesSummary,
  listDeals,
  listProducts,
  listSalesLeads,
  updateDeal,
  updateProduct,
} from '../api/sales'
import type { MessageKey } from '../i18n/messages'
import { usePreferences } from '../settings/PreferencesContext'
import {
  DEAL_STAGES,
  type Deal,
  type DealEvent,
  type DealStage,
  type Product,
  type SalesLead,
  type SalesSummary,
} from '../types'
import { IconPlus, IconRefresh, IconSales, IconTrash } from '../ui/icons'
import { Modal, useConfirm } from '../ui/Modal'
import { formatRelative } from '../ui/format'
import {
  Badge,
  EmptyState,
  Field,
  Notice,
  Progress,
  Segmented,
  Stat,
  Switch,
  Tabs,
  type Tone,
} from '../ui/primitives'

type Tab = 'overview' | 'pipeline' | 'products' | 'leads'
type Days = '7' | '30' | '90'

const STAGE_TONE: Record<DealStage, Tone> = {
  new: 'neutral',
  contacted: 'info',
  proposal: 'info',
  negotiation: 'warning',
  won: 'success',
  lost: 'error',
}

const LEAD_TONE: Record<SalesLead['stage'], Tone> = {
  cold: 'neutral',
  interested: 'info',
  qualified: 'warning',
  converted: 'success',
  lost: 'error',
}

interface DealForm {
  title: string
  product_id: string
  contact: string
  source: string
  quantity: string
  amount: string
  notes: string
}

const EMPTY_DEAL: DealForm = {
  title: '',
  product_id: '',
  contact: '',
  source: '',
  quantity: '1',
  amount: '',
  notes: '',
}

interface ProductForm {
  name: string
  description: string
  price: string
  cost: string
  currency: string
}

const EMPTY_PRODUCT: ProductForm = { name: '', description: '', price: '', cost: '', currency: 'RUB' }

function num(v: string): number | undefined {
  const n = Number(v.replace(',', '.'))
  return v.trim() && Number.isFinite(n) ? n : undefined
}

function errText(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}

export function leadName(lead: SalesLead): string {
  const m = lead.metadata ?? {}
  for (const key of ['name', 'username', 'title', 'contact']) {
    const v = m[key]
    if (typeof v === 'string' && v.trim()) return v
  }
  return lead.chat_id || lead.id.slice(0, 8)
}

export function SalesPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t, locale } = usePreferences()
  const confirm = useConfirm()
  const [tab, setTab] = useState<Tab>('overview')
  const [days, setDays] = useState<Days>('30')
  const [summary, setSummary] = useState<SalesSummary | null>(null)
  const [products, setProducts] = useState<Product[]>([])
  const [deals, setDeals] = useState<Deal[]>([])
  const [leads, setLeads] = useState<SalesLead[]>([])
  const [showArchived, setShowArchived] = useState(false)
  const [showSimulated, setShowSimulated] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)

  // Deal editor: null = closed, '' = new deal, otherwise the edited deal id.
  const [dealEditing, setDealEditing] = useState<string | null>(null)
  const [dealForm, setDealForm] = useState<DealForm>(EMPTY_DEAL)
  const [dealEvents, setDealEvents] = useState<DealEvent[]>([])

  const [productEditing, setProductEditing] = useState<string | null>(null)
  const [productForm, setProductForm] = useState<ProductForm>(EMPTY_PRODUCT)
  const [leadProduct, setLeadProduct] = useState('')

  const money = useCallback(
    (v: number, currency = summary?.currency || 'RUB') => {
      try {
        return new Intl.NumberFormat(locale, { style: 'currency', currency, maximumFractionDigits: 0 }).format(v)
      } catch {
        return `${Math.round(v).toLocaleString(locale)} ${currency}`
      }
    },
    [locale, summary?.currency],
  )
  const pct = (v: number) => `${Math.round(v * 100)}%`
  const stageLabel = (s: DealStage) => t(`sales.stage.${s}` as MessageKey)
  const leadStageLabel = (s: SalesLead['stage']) => t(`sales.leadStage.${s}` as MessageKey)

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const [s, p, d, l] = await Promise.all([
        getSalesSummary(Number(days)),
        listProducts(true),
        listDeals(),
        listSalesLeads(showSimulated),
      ])
      setSummary(s)
      setProducts(p)
      setDeals(d)
      setLeads(l)
    } catch (err) {
      setMessage(errText(err))
    } finally {
      setLoading(false)
    }
  }, [days, showSimulated])

  useEffect(() => {
    if (open) void reload()
  }, [open, reload])

  const productById = useMemo(() => new Map(products.map((p) => [p.id, p])), [products])
  const activeProducts = useMemo(() => products.filter((p) => p.active), [products])
  const dealsByLead = useMemo(() => {
    const m = new Map<string, number>()
    for (const d of deals) if (d.lead_id) m.set(d.lead_id, (m.get(d.lead_id) ?? 0) + 1)
    return m
  }, [deals])

  async function run(action: () => Promise<unknown>, ok?: string) {
    setBusy(true)
    try {
      await action()
      if (ok) setMessage(ok)
      await reload()
      return true
    } catch (err) {
      setMessage(errText(err))
      return false
    } finally {
      setBusy(false)
    }
  }

  // ----- deals -------------------------------------------------------------

  function openNewDeal() {
    setDealEditing('')
    setDealForm({ ...EMPTY_DEAL, product_id: activeProducts[0]?.id ?? '' })
    setDealEvents([])
  }

  async function openDeal(deal: Deal) {
    setDealEditing(deal.id)
    setDealForm({
      title: deal.title,
      product_id: deal.product_id ?? '',
      contact: deal.contact,
      source: deal.source,
      quantity: String(deal.quantity),
      amount: String(deal.amount),
      notes: deal.notes,
    })
    setDealEvents([])
    try {
      setDealEvents((await getDeal(deal.id)).events)
    } catch (err) {
      setMessage(errText(err))
    }
  }

  async function submitDeal(e: FormEvent) {
    e.preventDefault()
    if (dealEditing === null) return
    const quantity = num(dealForm.quantity)
    const amount = num(dealForm.amount)
    const ok = await run(async () => {
      if (dealEditing === '') {
        await createDeal({
          title: dealForm.title.trim(),
          product_id: dealForm.product_id || null,
          contact: dealForm.contact.trim(),
          source: dealForm.source.trim(),
          quantity: quantity ?? null,
          amount: amount ?? null,
          notes: dealForm.notes,
        })
      } else {
        await updateDeal(dealEditing, {
          title: dealForm.title.trim(),
          product_id: dealForm.product_id || undefined,
          contact: dealForm.contact.trim(),
          source: dealForm.source.trim(),
          quantity,
          amount,
          notes: dealForm.notes,
        })
      }
    }, t('sales.saved'))
    if (ok) setDealEditing(null)
  }

  async function moveDeal(deal: Deal, stage: DealStage) {
    if (stage === deal.stage) return
    await run(() => changeDealStage(deal.id, stage))
    if (dealEditing === deal.id) void openDeal({ ...deal, stage })
  }

  async function removeDeal(deal: Deal) {
    const ok = await confirm({
      title: t('sales.deleteDeal'),
      message: t('sales.confirmDeleteDeal', { name: deal.title }),
      confirmLabel: t('sales.delete'),
      danger: true,
    })
    if (!ok) return
    if (await run(() => deleteDeal(deal.id))) {
      if (dealEditing === deal.id) setDealEditing(null)
    }
  }

  // ----- products ----------------------------------------------------------

  function editProduct(p: Product | null) {
    setProductEditing(p ? p.id : '')
    setProductForm(
      p
        ? {
            name: p.name,
            description: p.description,
            price: String(p.price),
            cost: String(p.cost),
            currency: p.currency,
          }
        : EMPTY_PRODUCT,
    )
  }

  async function submitProduct(e: FormEvent) {
    e.preventDefault()
    if (productEditing === null) return
    const body = {
      name: productForm.name.trim(),
      description: productForm.description,
      price: num(productForm.price) ?? 0,
      cost: num(productForm.cost) ?? 0,
      currency: productForm.currency.trim() || null,
    }
    const ok = await run(
      () => (productEditing === '' ? createProduct(body) : updateProduct(productEditing, body)),
      t('sales.saved'),
    )
    if (ok) setProductEditing(null)
  }

  async function toggleProduct(p: Product) {
    await run(() =>
      updateProduct(p.id, {
        name: p.name,
        description: p.description,
        price: p.price,
        cost: p.cost,
        currency: p.currency,
        active: !p.active,
      }),
    )
  }

  async function removeProduct(p: Product) {
    const ok = await confirm({
      title: t('sales.deleteProduct'),
      message: t('sales.confirmDeleteProduct', { name: p.name }),
      confirmLabel: t('sales.delete'),
      danger: true,
    })
    if (!ok) return
    let archived = false
    await run(async () => {
      archived = Boolean(await deleteProduct(p.id))
    })
    if (archived) setMessage(t('sales.productArchived'))
  }

  // ----- leads -------------------------------------------------------------

  async function convertLead(lead: SalesLead) {
    const ok = await run(
      () => dealFromLead(lead.id, leadProduct ? { product_id: leadProduct } : {}),
      t('sales.dealCreated'),
    )
    if (ok) setTab('pipeline')
  }

  // ----- render ------------------------------------------------------------

  const visibleProducts = showArchived ? products : activeProducts

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      icon={<IconSales size={18} />}
      title={t('sales.title')}
      subtitle={t('sales.hint')}
      actions={
        <button type="button" className="icon-btn" onClick={() => void reload()} title={t('common.refresh')} aria-label={t('common.refresh')}>
          {loading ? <span className="spinner spinner--xs" /> : <IconRefresh size={15} />}
        </button>
      }
    >
      <div className="sales">
        <Tabs<Tab>
          value={tab}
          onChange={setTab}
          items={[
            { id: 'overview', label: t('sales.tab.overview') },
            { id: 'pipeline', label: t('sales.tab.pipeline'), count: summary?.totals.open },
            { id: 'products', label: t('sales.tab.products'), count: activeProducts.length },
            { id: 'leads', label: t('sales.tab.leads'), count: leads.length },
          ]}
        />
        <Notice text={message} onDismiss={() => setMessage(null)} />

        {tab === 'overview' && (
          <div className="sales__section">
            <div className="toolbar">
              <span className="eyebrow">{t('sales.period')}</span>
              <Segmented<Days>
                value={days}
                onChange={setDays}
                options={[
                  { value: '7', label: t('sales.days', { n: 7 }) },
                  { value: '30', label: t('sales.days', { n: 30 }) },
                  { value: '90', label: t('sales.days', { n: 90 }) },
                ]}
              />
            </div>
            {!summary ? (
              <EmptyState title={t('sales.loading')} compact />
            ) : (
              <>
                <div className="stat-grid">
                  <Stat label={t('sales.revenue')} value={money(summary.totals.revenue)} hint={t('sales.wonCount', { n: summary.totals.won })} tone="success" />
                  <Stat label={t('sales.grossProfit')} value={money(summary.totals.gross_profit)} />
                  <Stat label={t('sales.pipeline')} value={money(summary.totals.pipeline_value)} hint={t('sales.openCount', { n: summary.totals.open })} tone="info" />
                  <Stat label={t('sales.winRate')} value={pct(summary.totals.win_rate)} hint={t('sales.lostCount', { n: summary.totals.lost })} />
                  <Stat label={t('sales.avgDeal')} value={money(summary.totals.avg_deal)} />
                  <Stat label={t('sales.leadsTotal')} value={summary.leads.total} hint={`${t('sales.leadToSale')}: ${pct(summary.leads.lead_to_sale)}`} tone="warning" />
                </div>

                <div className="sales__grid">
                  <section className="sales__card">
                    <h4 className="sales__card-title">{t('sales.dailyRevenue')}</h4>
                    <RevenueChart daily={summary.daily} money={money} empty={t('sales.noRevenue')} />
                  </section>
                  <section className="sales__card">
                    <h4 className="sales__card-title">{t('sales.interest')}</h4>
                    <Funnel
                      rows={[
                        { label: t('sales.leadStage.all'), value: summary.leads.total },
                        { label: t('sales.leadStage.interested'), value: summary.leads.interested },
                        { label: t('sales.leadStage.qualified'), value: summary.leads.qualified },
                        { label: t('sales.leadStage.converted'), value: summary.leads.converted },
                      ]}
                    />
                    <p className="sales__muted">
                      {t('sales.leadToDeal')}: {pct(summary.leads.lead_to_deal)} · {t('sales.leadToSale')}:{' '}
                      {pct(summary.leads.lead_to_sale)}
                    </p>
                    {summary.leads.by_source.length > 0 && (
                      <div className="sales__chips">
                        {summary.leads.by_source.map((s) => (
                          <Badge key={s.name}>
                            {s.name || '—'}: {s.count}
                          </Badge>
                        ))}
                      </div>
                    )}
                  </section>
                </div>

                <h4 className="sales__card-title">{t('sales.byProduct')}</h4>
                {summary.by_product.length === 0 ? (
                  <EmptyState compact title={t('sales.noDeals')} />
                ) : (
                  <div className="table-wrap">
                    <table className="table">
                      <thead>
                        <tr>
                          <th>{t('sales.product')}</th>
                          <th>{t('sales.deals')}</th>
                          <th>{t('sales.won')}</th>
                          <th>{t('sales.units')}</th>
                          <th>{t('sales.revenue')}</th>
                          <th>{t('sales.grossProfit')}</th>
                          <th>{t('sales.pipeline')}</th>
                          <th>{t('sales.winRate')}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {summary.by_product.map((p) => (
                          <tr key={p.product_id ?? p.name}>
                            <td>{p.name}</td>
                            <td>{p.deals}</td>
                            <td>{p.won}</td>
                            <td>{p.units}</td>
                            <td>{money(p.revenue)}</td>
                            <td>{money(p.gross_profit)}</td>
                            <td>{money(p.pipeline_value)}</td>
                            <td>{pct(p.win_rate)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}

                {summary.by_source.length > 0 && (
                  <>
                    <h4 className="sales__card-title">{t('sales.bySource')}</h4>
                    <div className="table-wrap">
                      <table className="table">
                        <thead>
                          <tr>
                            <th>{t('sales.source')}</th>
                            <th>{t('sales.deals')}</th>
                            <th>{t('sales.won')}</th>
                            <th>{t('sales.revenue')}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {summary.by_source.map((s) => (
                            <tr key={s.source}>
                              <td>{s.source || '—'}</td>
                              <td>{s.deals}</td>
                              <td>{s.won}</td>
                              <td>{money(s.revenue)}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        )}

        {tab === 'pipeline' && (
          <div className="sales__section">
            <div className="toolbar">
              <button type="button" className="btn btn--primary btn--sm" onClick={openNewDeal}>
                <IconPlus size={13} />
                {t('sales.newDeal')}
              </button>
              <span className="toolbar__spacer" />
              {summary && (
                <span className="sales__muted">
                  {t('sales.pipeline')}: {money(summary.totals.pipeline_value)}
                </span>
              )}
            </div>

            {dealEditing !== null && (
              <form className="form form--inset" onSubmit={(e) => void submitDeal(e)}>
                <span className="eyebrow">{dealEditing === '' ? t('sales.newDeal') : t('sales.editDeal')}</span>
                <div className="form__row">
                  <Field label={t('sales.dealTitle')} hint={t('sales.dealTitleHint')}>
                    <input value={dealForm.title} onChange={(e) => setDealForm({ ...dealForm, title: e.target.value })} />
                  </Field>
                  <Field label={t('sales.product')}>
                    <select value={dealForm.product_id} onChange={(e) => setDealForm({ ...dealForm, product_id: e.target.value })}>
                      <option value="">{t('sales.noProduct')}</option>
                      {products
                        .filter((p) => p.active || p.id === dealForm.product_id)
                        .map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.name} · {money(p.price, p.currency)}
                          </option>
                        ))}
                    </select>
                  </Field>
                </div>
                <div className="form__row form__row--4">
                  <Field label={t('sales.contact')}>
                    <input value={dealForm.contact} onChange={(e) => setDealForm({ ...dealForm, contact: e.target.value })} />
                  </Field>
                  <Field label={t('sales.source')}>
                    <input value={dealForm.source} placeholder="telegram" onChange={(e) => setDealForm({ ...dealForm, source: e.target.value })} />
                  </Field>
                  <Field label={t('sales.units')}>
                    <input inputMode="numeric" value={dealForm.quantity} onChange={(e) => setDealForm({ ...dealForm, quantity: e.target.value })} />
                  </Field>
                  <Field label={t('sales.amount')} hint={t('sales.amountHint')}>
                    <input inputMode="decimal" value={dealForm.amount} onChange={(e) => setDealForm({ ...dealForm, amount: e.target.value })} />
                  </Field>
                </div>
                <Field label={t('sales.notes')} span="full">
                  <textarea rows={2} value={dealForm.notes} onChange={(e) => setDealForm({ ...dealForm, notes: e.target.value })} />
                </Field>
                {dealEvents.length > 0 && (
                  <ul className="sales__events">
                    {dealEvents.map((ev) => (
                      <li key={ev.id}>
                        <span className="sales__muted">{formatRelative(ev.at, locale)}</span>{' '}
                        {ev.to_stage
                          ? `${ev.from_stage ? stageLabel(ev.from_stage) : '—'} → ${stageLabel(ev.to_stage)}`
                          : t(`sales.event.${ev.kind === 'created' ? 'created' : 'updated'}` as MessageKey)}
                        {ev.note ? ` · ${ev.note}` : ''}
                      </li>
                    ))}
                  </ul>
                )}
                <div className="form__actions">
                  <button type="submit" className="btn btn--primary btn--sm" disabled={busy}>
                    {t('sales.save')}
                  </button>
                  <button type="button" className="btn btn--sm" onClick={() => setDealEditing(null)}>
                    {t('common.cancel')}
                  </button>
                </div>
              </form>
            )}

            {deals.length === 0 ? (
              <EmptyState
                title={t('sales.noDeals')}
                hint={t('sales.noDealsHint')}
                action={
                  <button type="button" className="btn btn--primary btn--sm" onClick={openNewDeal}>
                    {t('sales.newDeal')}
                  </button>
                }
              />
            ) : (
              <div className="sales-board">
                {DEAL_STAGES.map((stage) => {
                  const column = deals.filter((d) => d.stage === stage)
                  const total = column.reduce((acc, d) => acc + d.amount, 0)
                  return (
                    <section key={stage} className={`sales-board__col sales-board__col--${stage}`}>
                      <header className="sales-board__head">
                        <Badge tone={STAGE_TONE[stage]} dot>
                          {stageLabel(stage)}
                        </Badge>
                        <span className="sales__muted">
                          {column.length} · {money(total)}
                        </span>
                      </header>
                      {column.map((d) => (
                        <article key={d.id} className={`deal-card${dealEditing === d.id ? ' is-active' : ''}`}>
                          <button type="button" className="deal-card__main" onClick={() => void openDeal(d)}>
                            <strong>{d.title}</strong>
                            <span className="deal-card__amount">{money(d.amount, d.currency)}</span>
                            <span className="sales__muted">
                              {[d.product_id ? productById.get(d.product_id)?.name ?? t('sales.unknownProduct') : null, d.contact, d.source]
                                .filter(Boolean)
                                .join(' · ')}
                            </span>
                          </button>
                          <div className="deal-card__actions">
                            <select
                              value={d.stage}
                              disabled={busy}
                              aria-label={t('sales.stage')}
                              onChange={(e) => void moveDeal(d, e.target.value as DealStage)}
                            >
                              {DEAL_STAGES.map((s) => (
                                <option key={s} value={s}>
                                  {stageLabel(s)}
                                </option>
                              ))}
                            </select>
                            <button
                              type="button"
                              className="icon-btn icon-btn--sm icon-btn--danger"
                              onClick={() => void removeDeal(d)}
                              title={t('sales.deleteDeal')}
                              aria-label={t('sales.deleteDeal')}
                            >
                              <IconTrash size={13} />
                            </button>
                          </div>
                        </article>
                      ))}
                    </section>
                  )
                })}
              </div>
            )}
          </div>
        )}

        {tab === 'products' && (
          <div className="sales__section">
            <div className="toolbar">
              <button type="button" className="btn btn--primary btn--sm" onClick={() => editProduct(null)}>
                <IconPlus size={13} />
                {t('sales.newProduct')}
              </button>
              <span className="toolbar__spacer" />
              <Switch checked={showArchived} onChange={setShowArchived} label={t('sales.showArchived')} />
            </div>

            {productEditing !== null && (
              <form className="form form--inset" onSubmit={(e) => void submitProduct(e)}>
                <span className="eyebrow">{productEditing === '' ? t('sales.newProduct') : t('sales.editProduct')}</span>
                <div className="form__row form__row--4">
                  <Field label={t('sales.productName')}>
                    <input required value={productForm.name} onChange={(e) => setProductForm({ ...productForm, name: e.target.value })} />
                  </Field>
                  <Field label={t('sales.price')}>
                    <input inputMode="decimal" value={productForm.price} onChange={(e) => setProductForm({ ...productForm, price: e.target.value })} />
                  </Field>
                  <Field label={t('sales.cost')} hint={t('sales.costHint')}>
                    <input inputMode="decimal" value={productForm.cost} onChange={(e) => setProductForm({ ...productForm, cost: e.target.value })} />
                  </Field>
                  <Field label={t('sales.currency')}>
                    <input maxLength={3} value={productForm.currency} onChange={(e) => setProductForm({ ...productForm, currency: e.target.value.toUpperCase() })} />
                  </Field>
                </div>
                <Field label={t('sales.description')} span="full">
                  <textarea rows={2} value={productForm.description} onChange={(e) => setProductForm({ ...productForm, description: e.target.value })} />
                </Field>
                <div className="form__actions">
                  <button type="submit" className="btn btn--primary btn--sm" disabled={busy || !productForm.name.trim()}>
                    {t('sales.save')}
                  </button>
                  <button type="button" className="btn btn--sm" onClick={() => setProductEditing(null)}>
                    {t('common.cancel')}
                  </button>
                </div>
              </form>
            )}

            {visibleProducts.length === 0 ? (
              <EmptyState title={t('sales.noProducts')} hint={t('sales.noProductsHint')} />
            ) : (
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>{t('sales.productName')}</th>
                      <th>{t('sales.price')}</th>
                      <th>{t('sales.cost')}</th>
                      <th>{t('sales.margin')}</th>
                      <th>{t('sales.deals')}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {visibleProducts.map((p) => {
                      const margin = p.price > 0 ? (p.price - p.cost) / p.price : 0
                      const count = deals.filter((d) => d.product_id === p.id).length
                      return (
                        <tr key={p.id}>
                          <td>
                            <strong>{p.name}</strong> {!p.active && <Badge>{t('sales.archived')}</Badge>}
                            {p.description && <div className="table__sub">{p.description}</div>}
                          </td>
                          <td>{money(p.price, p.currency)}</td>
                          <td>{money(p.cost, p.currency)}</td>
                          <td>{pct(margin)}</td>
                          <td>{count}</td>
                          <td className="sales__row-actions">
                            <button type="button" className="btn btn--xs" onClick={() => editProduct(p)}>
                              {t('sales.edit')}
                            </button>
                            <button type="button" className="btn btn--xs" disabled={busy} onClick={() => void toggleProduct(p)}>
                              {p.active ? t('sales.archive') : t('sales.restore')}
                            </button>
                            <button
                              type="button"
                              className="icon-btn icon-btn--sm icon-btn--danger"
                              onClick={() => void removeProduct(p)}
                              title={t('sales.deleteProduct')}
                              aria-label={t('sales.deleteProduct')}
                            >
                              <IconTrash size={13} />
                            </button>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        )}

        {tab === 'leads' && (
          <div className="sales__section">
            <div className="toolbar">
              <Field label={t('sales.leadProduct')}>
                <select value={leadProduct} onChange={(e) => setLeadProduct(e.target.value)}>
                  <option value="">{t('sales.noProduct')}</option>
                  {activeProducts.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </Field>
              <span className="toolbar__spacer" />
              <Switch checked={showSimulated} onChange={setShowSimulated} label={t('sales.showSimulated')} />
            </div>
            {leads.length === 0 ? (
              <EmptyState title={t('sales.noLeads')} hint={t('sales.noLeadsHint')} />
            ) : (
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>{t('sales.lead')}</th>
                      <th>{t('sales.source')}</th>
                      <th>{t('sales.stage')}</th>
                      <th>{t('sales.score')}</th>
                      <th>{t('sales.lastActivity')}</th>
                      <th>{t('sales.deals')}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {leads.map((l) => (
                      <tr key={l.id}>
                        <td>
                          <strong>{leadName(l)}</strong> {l.simulated && <Badge tone="warning">{t('sales.simulated')}</Badge>}
                          {l.score_reasons.length > 0 && <div className="table__sub">{l.score_reasons.join(', ')}</div>}
                        </td>
                        <td>{l.source}</td>
                        <td>
                          <Badge tone={LEAD_TONE[l.stage]} dot>
                            {leadStageLabel(l.stage)}
                          </Badge>
                        </td>
                        <td>{l.score}</td>
                        <td>{formatRelative(l.last_activity_at, locale)}</td>
                        <td>{dealsByLead.get(l.id) ?? 0}</td>
                        <td className="sales__row-actions">
                          <button type="button" className="btn btn--xs" disabled={busy} onClick={() => void convertLead(l)}>
                            {t('sales.toDeal')}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        )}
      </div>
    </Modal>
  )
}

function RevenueChart({
  daily,
  money,
  empty,
}: {
  daily: SalesSummary['daily']
  money: (v: number) => string
  empty: string
}) {
  const max = Math.max(0, ...daily.map((d) => d.revenue))
  if (max <= 0) return <p className="sales__muted">{empty}</p>
  return (
    <div className="sales-chart" role="img">
      {daily.map((d) => (
        <span
          key={d.date}
          className="sales-chart__bar"
          style={{ height: `${Math.max(2, (d.revenue / max) * 100)}%` }}
          title={`${d.date}: ${money(d.revenue)} · +${d.new_deals} / ✓${d.won}`}
        />
      ))}
    </div>
  )
}

function Funnel({ rows }: { rows: { label: string; value: number }[] }) {
  const top = Math.max(1, rows[0]?.value ?? 0)
  return (
    <div className="sales-funnel">
      {rows.map((r) => (
        <div key={r.label} className="sales-funnel__row">
          <span>{r.label}</span>
          <Progress value={(r.value / top) * 100} />
          <strong>{r.value}</strong>
        </div>
      ))}
    </div>
  )
}
