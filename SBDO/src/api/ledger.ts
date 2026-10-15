import { request } from './client'
import type { LedgerEntry, LedgerFilter, LedgerSummary } from '../types'

function query(params: Record<string, string | number | null | undefined>) {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
  }
  const s = q.toString()
  return s ? `?${s}` : ''
}

export function listLedger(filter: LedgerFilter = {}) {
  return request<LedgerEntry[]>(`/api/ledger${query({ ...filter })}`)
}

export function getLedgerSummary(filter: LedgerFilter = {}) {
  const { limit: _limit, ...rest } = filter
  return request<LedgerSummary>(`/api/ledger/summary${query({ ...rest })}`)
}
