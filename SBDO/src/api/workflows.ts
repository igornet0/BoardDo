import { request } from './client'
import type {
  CreateWorkflowRequest,
  RunWorkflowResponse,
  UpdateWorkflowRequest,
  ValidateResponse,
  WorkflowRecord,
  WorkflowSummary,
} from '../types'

export function getWorkflows() {
  return request<WorkflowSummary[]>('/api/workflows')
}

export function getWorkflow(id: string) {
  return request<WorkflowRecord>(`/api/workflows/${id}`)
}

export function createWorkflow(body: CreateWorkflowRequest) {
  return request<WorkflowRecord>('/api/workflows', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function updateWorkflow(id: string, body: UpdateWorkflowRequest) {
  return request<WorkflowRecord>(`/api/workflows/${id}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteWorkflow(id: string) {
  return request<void>(`/api/workflows/${id}`, { method: 'DELETE' })
}

export function validateWorkflow(id: string) {
  return request<ValidateResponse>(`/api/workflows/${id}/validate`, {
    method: 'POST',
  })
}

export function runWorkflow(id: string, trigger: unknown = {}) {
  return request<RunWorkflowResponse>(`/api/workflows/${id}/run`, {
    method: 'POST',
    body: JSON.stringify({ trigger }),
  })
}
