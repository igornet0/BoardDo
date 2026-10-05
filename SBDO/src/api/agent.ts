import { request } from './client'

export interface AiModelCapabilities {
  chat?: boolean
  image_generate?: boolean
  image_edit?: boolean
  video_generate?: boolean
  audio_generate?: boolean
  audio_transcribe?: boolean
}

export interface AiModelEntry {
  id: string
  label?: string | null
  capabilities?: AiModelCapabilities
}

export interface AgentSettings {
  enabled: boolean
  connection_id?: string | null
  model?: string | null
  has_connection: boolean
  connection_name?: string | null
  base_url?: string | null
  default_model?: string | null
}

export interface UpdateAgentSettingsRequest {
  enabled: boolean
  connection_id?: string | null
  model?: string | null
}

export interface AgentSettingsTestResponse {
  ok: boolean
  message: string
}

export interface AgentChatMessage {
  role: 'user' | 'assistant' | string
  content: string
}

export interface AgentWorkflowSnapshot {
  name: string
  description: string
  nodes: {
    id: string
    type_id: string
    category?: string | null
    position: { x: number; y: number }
    config: Record<string, unknown>
  }[]
  edges: {
    id: string
    source: string
    target: string
    source_port?: string | null
    target_port?: string | null
  }[]
}

export type AgentGraphOp =
  | {
      op: 'add_node'
      id?: string | null
      type_id: string
      position?: { x: number; y: number } | null
      config?: Record<string, unknown>
    }
  | {
      op: 'update_node'
      id: string
      config?: Record<string, unknown> | null
      position?: { x: number; y: number } | null
    }
  | { op: 'remove_node'; id: string }
  | {
      op: 'add_edge'
      id?: string | null
      source: string
      target: string
      source_port?: string | null
      target_port?: string | null
    }
  | {
      op: 'remove_edge'
      id?: string | null
      source?: string | null
      target?: string | null
    }
  | {
      op: 'set_meta'
      name?: string | null
      description?: string | null
    }

export interface AgentChatRequest {
  messages: AgentChatMessage[]
  workflow: AgentWorkflowSnapshot
  selection?: { node_ids: string[] }
  locale?: string
}

export interface AgentChatResponse {
  message: string
  ops: AgentGraphOp[]
  model: string
  usage?: {
    prompt_tokens?: number
    completion_tokens?: number
    total_tokens?: number
  } | null
}

export function getAgentSettings() {
  return request<AgentSettings>('/api/agent/settings')
}

export function updateAgentSettings(body: UpdateAgentSettingsRequest) {
  return request<AgentSettings>('/api/agent/settings', {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function testAgentSettings() {
  return request<AgentSettingsTestResponse>('/api/agent/settings/test', {
    method: 'POST',
    body: '{}',
  })
}

export function agentChat(body: AgentChatRequest) {
  return request<AgentChatResponse>('/api/agent/chat', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}
