import { request } from './client'
import type { Connection, CreateConnectionRequest, UpdateConnectionRequest } from '../types'

export function getConnections() {
  return request<Connection[]>('/api/connections')
}

export function getConnection(id: string) {
  return request<Connection>(`/api/connections/${id}`)
}

export function createConnection(body: CreateConnectionRequest) {
  return request<Connection>('/api/connections', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function updateConnection(id: string, body: UpdateConnectionRequest) {
  return request<Connection>(`/api/connections/${id}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteConnection(id: string) {
  return request<void>(`/api/connections/${id}`, { method: 'DELETE' })
}

export function testConnection(id: string) {
  return request<{ ok: boolean; message: string }>(
    `/api/connections/${id}/test`,
    { method: 'POST' },
  )
}
