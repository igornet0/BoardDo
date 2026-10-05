import { request } from './client'
import type {
  AgentApproval,
  AgentAuditEntry,
  AgentMemoryEntry,
  AgentTemplate,
  CampaignCreatedResponse,
  CampaignPlan,
  CreateCampaignRequest,
  CreateGoalRequest,
  Experiment,
  GoalDetail,
  GoalRun,
  GoalSpec,
  MarketingSnapshot,
  ContentEngineSnapshot,
} from '../types'

export function listGoals() {
  return request<GoalDetail[]>('/api/goals')
}

export function createGoal(body: CreateGoalRequest) {
  return request<GoalSpec>('/api/goals', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function createCampaign(body: CreateCampaignRequest) {
  return request<CampaignCreatedResponse>('/api/goals/campaigns', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function getCampaignPlan(goalId: string) {
  return request<CampaignPlan>(`/api/goals/${goalId}/plan`)
}

export function getGoal(id: string) {
  return request<GoalDetail>(`/api/goals/${id}`)
}

export function deleteGoal(id: string) {
  return request<void>(`/api/goals/${id}`, { method: 'DELETE' })
}

export function startGoal(id: string) {
  return request<GoalRun>(`/api/goals/${id}/start`, { method: 'POST', body: '{}' })
}

export function stopGoal(id: string) {
  return request<GoalRun>(`/api/goals/${id}/stop`, { method: 'POST' })
}

export function pauseGoal(id: string) {
  return request<GoalRun>(`/api/goals/${id}/pause`, { method: 'POST' })
}

export function recordGoalEvent(
  id: string,
  event_type: string,
  payload: Record<string, unknown> = {},
) {
  return request<GoalRun>(`/api/goals/${id}/events`, {
    method: 'POST',
    body: JSON.stringify({ type: event_type, payload }),
  })
}

export function listGoalActivity(runId: string) {
  return request<{ entries: AgentAuditEntry[] }>(
    `/api/goal-runs/${runId}/activity`,
  )
}

export function listExperiments(runId: string) {
  return request<{ experiments: Experiment[] }>(
    `/api/goal-runs/${runId}/experiments`,
  )
}

export function listGoalMemory(runId: string) {
  return request<{ memory: AgentMemoryEntry[] }>(
    `/api/goal-runs/${runId}/memory`,
  )
}

export function getMarketingSnapshot(runId: string) {
  return request<MarketingSnapshot>(`/api/goal-runs/${runId}/marketing`)
}

export function getContentEngineSnapshot(runId: string) {
  return request<ContentEngineSnapshot>(`/api/goal-runs/${runId}/content-engine`)
}

export function listApprovals(runId?: string, pending = false) {
  const q = new URLSearchParams()
  if (runId) q.set('run_id', runId)
  if (pending) q.set('pending', 'true')
  const suffix = q.toString() ? `?${q}` : ''
  return request<AgentApproval[]>(`/api/approvals${suffix}`)
}

export function approveAction(id: string) {
  return request<AgentApproval>(`/api/approvals/${id}/approve`, {
    method: 'POST',
  })
}

export function rejectAction(id: string) {
  return request<AgentApproval>(`/api/approvals/${id}/reject`, {
    method: 'POST',
  })
}

export function listAgentTemplates() {
  return request<{ templates: AgentTemplate[] }>('/api/agent-templates')
}

export function goalsWsUrl(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${window.location.host}/ws/goals`
}
