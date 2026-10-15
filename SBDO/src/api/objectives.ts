import { request } from './client'
import type {
  Objective,
  ObjectiveAssistRequest,
  ObjectiveAssistResponse,
  ObjectiveInput,
  RunObjectiveRequest,
  UpdateObjectiveItemRequest,
} from '../types'

export function listObjectives() {
  return request<Objective[]>('/api/objectives')
}

export function getObjective(id: string) {
  return request<Objective>(`/api/objectives/${id}`)
}

export function createObjective(body: ObjectiveInput) {
  return request<Objective>('/api/objectives', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function updateObjective(id: string, body: ObjectiveInput) {
  return request<Objective>(`/api/objectives/${id}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteObjective(id: string) {
  return request<void>(`/api/objectives/${id}`, { method: 'DELETE' })
}

export function assistObjective(id: string, body: ObjectiveAssistRequest) {
  return request<ObjectiveAssistResponse>(`/api/objectives/${id}/assist`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function runObjective(id: string, body: RunObjectiveRequest = {}) {
  return request<Objective>(`/api/objectives/${id}/run`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function stopObjective(id: string) {
  return request<Objective>(`/api/objectives/${id}/stop`, { method: 'POST' })
}

export function updateObjectiveItem(id: string, itemId: string, body: UpdateObjectiveItemRequest) {
  return request<Objective>(`/api/objectives/${id}/items/${encodeURIComponent(itemId)}`, {
    method: 'PATCH',
    body: JSON.stringify(body),
  })
}

export function ensureItemScenario(id: string, itemId: string) {
  return request<Objective>(
    `/api/objectives/${id}/items/${encodeURIComponent(itemId)}/scenario`,
    { method: 'POST' },
  )
}

/** Same socket as campaigns; objective events carry `type: "objective_updated"`. */
export function objectivesWsUrl(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${window.location.host}/ws/goals`
}
