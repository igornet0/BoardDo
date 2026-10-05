import { request } from './client'

export type ToolSummary = {
  id: string
  name: string
  description: string
  language: string
  origin: string
  created_by: string
  active_version_id?: string | null
  tags: string[]
  execution_stats: {
    execution_count: number
    success_count: number
    failure_count: number
    average_duration_ms: number
    last_error?: string | null
  }
  created_at: string
  updated_at: string
}

export async function listTools(): Promise<{ tools: ToolSummary[] }> {
  return request('/api/tools')
}

export async function getTool(id: string): Promise<unknown> {
  return request(`/api/tools/${id}`)
}

export async function createTool(body: {
  name: string
  description: string
  code: string
  output_schema?: unknown
  test_input?: unknown
}): Promise<unknown> {
  return request('/api/tools', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function runTool(
  name: string,
  input: unknown,
): Promise<unknown> {
  return request(`/api/tools/${encodeURIComponent(name)}/run`, {
    method: 'POST',
    body: JSON.stringify({ input }),
  })
}
