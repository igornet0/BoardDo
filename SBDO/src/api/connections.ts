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

/** AI connection types (OpenAI-compatible gateways and Claude). */
export const LLM_CONNECTION_TYPES = ['openai', 'ai', 'anthropic', 'claude'] as const

export function isLlmConnection(c: Pick<Connection, 'type'>): boolean {
  return (LLM_CONNECTION_TYPES as readonly string[]).includes(c.type)
}

export function isClaudeConnection(c: Pick<Connection, 'type' | 'config'>): boolean {
  const cfg = (c.config ?? {}) as Record<string, unknown>
  return (
    c.type === 'anthropic' ||
    c.type === 'claude' ||
    cfg.provider === 'anthropic' ||
    String(cfg.base_url ?? '').includes('anthropic.com')
  )
}

export const CLAUDE_BASE_URL = 'https://api.anthropic.com/v1'
export const CLAUDE_DEFAULT_MODEL = 'claude-opus-5-5'

const CHAT_ONLY = {
  chat: true,
  image_generate: false,
  image_edit: false,
  video_generate: false,
  audio_generate: false,
  audio_transcribe: false,
}

export const CLAUDE_MODELS = [
  { id: 'claude-opus-5-5', label: 'Claude Opus 5.5', capabilities: CHAT_ONLY },
  { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5', capabilities: CHAT_ONLY },
  { id: 'claude-haiku-4-5', label: 'Claude Haiku 4.5', capabilities: CHAT_ONLY },
  { id: 'claude-fable-5-1', label: 'Claude Fable 5.1', capabilities: CHAT_ONLY },
]
