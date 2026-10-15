import { request } from './client'
import type {
  CreateDealRequest,
  Deal,
  DealDetails,
  DealStage,
  Product,
  ProductInput,
  SalesLead,
  SalesSummary,
  UpdateDealRequest,
} from '../types'

function query(params: Record<string, string | number | boolean | null | undefined>) {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '' && v !== false) q.set(k, String(v))
  }
  const s = q.toString()
  return s ? `?${s}` : ''
}

// ----- products -------------------------------------------------------------

export function listProducts(includeInactive = false) {
  return request<Product[]>(`/api/products${query({ include_inactive: includeInactive })}`)
}

export function createProduct(body: ProductInput) {
  return request<Product>('/api/products', { method: 'POST', body: JSON.stringify(body) })
}

export function updateProduct(id: string, body: ProductInput) {
  return request<Product>(`/api/products/${id}`, { method: 'PUT', body: JSON.stringify(body) })
}

/** Returns the archived product when it has deals, `undefined` when deleted. */
export function deleteProduct(id: string) {
  return request<Product | undefined>(`/api/products/${id}`, { method: 'DELETE' })
}

// ----- deals ----------------------------------------------------------------

export interface DealListFilter {
  stage?: DealStage
  product_id?: string
  lead_id?: string
}

export function listDeals(filter: DealListFilter = {}) {
  return request<Deal[]>(`/api/deals${query({ ...filter })}`)
}

export function getDeal(id: string) {
  return request<DealDetails>(`/api/deals/${id}`)
}

export function createDeal(body: CreateDealRequest) {
  return request<Deal>('/api/deals', { method: 'POST', body: JSON.stringify(body) })
}

export function updateDeal(id: string, body: UpdateDealRequest) {
  return request<Deal>(`/api/deals/${id}`, { method: 'PATCH', body: JSON.stringify(body) })
}

export function deleteDeal(id: string) {
  return request<void>(`/api/deals/${id}`, { method: 'DELETE' })
}

export function changeDealStage(id: string, stage: DealStage, note?: string) {
  return request<Deal>(`/api/deals/${id}/stage`, {
    method: 'POST',
    body: JSON.stringify({ stage, note: note || null }),
  })
}

export function dealFromLead(leadId: string, body: CreateDealRequest = {}) {
  return request<Deal>(`/api/deals/from-lead/${leadId}`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

// ----- analytics ------------------------------------------------------------

export function listSalesLeads(includeSimulated = false) {
  return request<SalesLead[]>(`/api/sales/leads${query({ include_simulated: includeSimulated })}`)
}

export function getSalesSummary(days = 30) {
  return request<SalesSummary>(`/api/sales/summary${query({ days })}`)
}
