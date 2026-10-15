import { request } from './client'

export type ToolPermissions = {
  network: boolean
  filesystem_read: boolean
  filesystem_write: boolean
  shell: boolean
  database: boolean
}

export type ToolTestCase = {
  input: unknown
  expected_schema?: unknown
  expected?: unknown
}

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

export type ToolRepair = {
  attempt: number
  error_message: string
  traceback?: string | null
  fixed_by?: 'auto' | 'ai' | null
  at: string
}

export type ToolVersion = {
  id: string
  version: number
  source_code: string
  input_schema: unknown
  output_schema: unknown
  permissions: ToolPermissions
  requirements: string[]
  runtime_config: { timeout_secs: number; memory_mb: number; max_stdout_kb: number }
  status: 'ACTIVE' | 'FAILED' | 'DRAFT' | 'TESTING' | 'DISABLED' | 'ARCHIVED'
  purpose: string
  test_cases: ToolTestCase[]
  repair_history: ToolRepair[]
  created_at: string
}

export type ToolExecution = {
  id: string
  version: number
  status: 'success' | 'failed'
  duration_ms: number
  output?: unknown
  error_type?: string | null
  message?: string | null
  traceback?: string | null
  logs: string
  created_at: string
}

export type ToolDetail = {
  tool: ToolSummary
  versions: ToolVersion[]
  executions: ToolExecution[]
}

/** Build report returned with a failed creation (`ApiError.body.details`). */
export type BuildReport = {
  activated: boolean
  failure?: string | null
  error_type?: string | null
  message?: string | null
  traceback?: string | null
  logs?: string
  repairs: ToolRepair[]
  tests: number
  code: string
}

export type ToolRunResult = {
  execution_id: string
  tool_id: string
  version: number
  status: 'success' | 'failed'
  duration_ms?: number
  output?: unknown
  error_type?: string | null
  message?: string | null
  traceback?: string | null
  logs: string
}

export type CreateToolBody = {
  name: string
  description: string
  code: string
  input_schema?: unknown
  output_schema?: unknown
  test_input?: unknown
  tests?: ToolTestCase[]
  permissions?: Partial<ToolPermissions>
  requirements?: string[]
}

export type GenerateToolBody = {
  description: string
  name?: string
  examples?: ToolTestCase[]
  permissions?: Partial<ToolPermissions>
  connection_id?: string
  model?: string
}

export type CreatedTool = { tool: ToolSummary; version: ToolVersion; generated?: { model: string } }

const enc = encodeURIComponent

export const listTools = (): Promise<{ tools: ToolSummary[] }> => request('/api/tools')

export const getTool = (idOrName: string): Promise<ToolDetail> => request(`/api/tools/${enc(idOrName)}`)

export const createTool = (body: CreateToolBody): Promise<CreatedTool> =>
  request('/api/tools', { method: 'POST', body: JSON.stringify(body) })

/** AI writes the tool from a description, tests it and repairs failures. */
export const generateTool = (body: GenerateToolBody): Promise<CreatedTool> =>
  request('/api/tools/generate', { method: 'POST', body: JSON.stringify(body) })

/** New version; a failing build keeps the active one (`rolled_back: true`). */
export const updateTool = (
  idOrName: string,
  body: CreateToolBody,
): Promise<CreatedTool & { rolled_back: boolean; report: BuildReport }> =>
  request(`/api/tools/${enc(idOrName)}`, { method: 'PUT', body: JSON.stringify(body) })

export const runTool = (idOrName: string, input: unknown): Promise<ToolRunResult> =>
  request(`/api/tools/${enc(idOrName)}/run`, { method: 'POST', body: JSON.stringify({ input }) })

export const deleteTool = (idOrName: string): Promise<{ deleted: string }> =>
  request(`/api/tools/${enc(idOrName)}`, { method: 'DELETE' })

// ─── Python runtime ──────────────────────────────────────────────────────────

export type PythonInterpreter = {
  path: string
  executable: string
  version: string
  /** import name → installed */
  packages: Record<string, boolean>
  prefix: string
  venv: boolean
  /** Standard modules that fail to import; empty — healthy. */
  broken: string[]
}

export type ToolsRuntime = {
  current: PythonInterpreter | null
  /** `auto` — the newest healthy interpreter is chosen on every start. */
  mode: 'auto' | 'manual'
  automatic: PythonInterpreter | null
  candidates: PythonInterpreter[]
  venv: string | null
  packages: { pip: string; import: string }[]
}

export const getToolsRuntime = (): Promise<ToolsRuntime> => request('/api/tools/runtime')

/** `null` — automatic (the newest interpreter found). */
export const setToolsPython = (python: string | null): Promise<ToolsRuntime> =>
  request('/api/tools/runtime', { method: 'PUT', body: JSON.stringify({ python }) })

/** Create the BoardDo tools venv from `python` and install packages (downloads from PyPI). */
export const createToolsVenv = (body: {
  python?: string
  packages: string[]
}): Promise<{ interpreter: PythonInterpreter; venv: string; installed: string[]; log: string }> =>
  request('/api/tools/runtime/venv', { method: 'POST', body: JSON.stringify(body) })
