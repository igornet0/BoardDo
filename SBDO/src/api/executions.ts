import { request } from './client'
import type { Execution, ExecutionChangelog, NodeExecution } from '../types'

export function getExecutions() {
  return request<Execution[]>('/api/executions')
}

export function getExecution(id: string) {
  return request<Execution>(`/api/executions/${id}`)
}

export function getExecutionNodes(id: string) {
  return request<NodeExecution[]>(`/api/executions/${id}/nodes`)
}

export function getExecutionChangelog(id: string) {
  return request<ExecutionChangelog>(`/api/executions/${id}/changelog`)
}

export function listExecutionChangelogs(params?: {
  workflowId?: string
  limit?: number
}) {
  const q = new URLSearchParams()
  if (params?.workflowId) q.set('workflow_id', params.workflowId)
  if (params?.limit != null) q.set('limit', String(params.limit))
  const suffix = q.toString() ? `?${q}` : ''
  return request<ExecutionChangelog[]>(`/api/executions/changelogs${suffix}`)
}
