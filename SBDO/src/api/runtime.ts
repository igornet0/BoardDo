import { request } from './client'
import type { RuntimeSnapshot } from '../types'

export function getRuntime(workflowId: string) {
  return request<RuntimeSnapshot>(`/api/workflows/${workflowId}/runtime`)
}

export function listRuntimes() {
  return request<RuntimeSnapshot[]>('/api/runtimes')
}

export function startRuntime(workflowId: string) {
  return request<RuntimeSnapshot>(`/api/workflows/${workflowId}/runtime/start`, {
    method: 'POST',
  })
}

export function stopRuntime(workflowId: string) {
  return request<RuntimeSnapshot>(`/api/workflows/${workflowId}/runtime/stop`, {
    method: 'POST',
  })
}

export function reloadRuntime(workflowId: string) {
  return request<RuntimeSnapshot>(
    `/api/workflows/${workflowId}/runtime/reload`,
    { method: 'POST' },
  )
}
