import { request } from './client'

// ─── Extension lifecycle ────────────────────────────────────────────────────

export type ExtensionState =
  | 'not_installed'
  | 'installing'
  | 'installed'
  | 'starting'
  | 'running'
  | 'error'
  /** Not adopted yet, but a Project Hub is installed on this machine. */
  | 'detected'
  /** Adopted, but its binary is gone (app removed / moved). */
  | 'missing'

export interface SelfTestStep {
  name: string
  ok: boolean
  ms: number
  detail?: string
}

export interface SelfTestReport {
  ok: boolean
  version: string
  passed: number
  failed: number
  duration_ms: number
  steps: SelfTestStep[]
}

export interface ProjectHubConfig {
  installed: boolean
  enabled: boolean
  repo: string
  branch: string
  source_dir?: string | null
  binary?: string | null
  version?: string | null
  installed_at?: string | null
  self_test?: SelfTestReport | null
  commands: 'off' | 'detected' | 'any'
  db_path?: string | null
  /** build = BoardDo built it; app / env / path / managed = adopted installation. */
  source?: InstallSource | null
  ignore_detected?: boolean
}

export type InstallSource = 'build' | 'app' | 'env' | 'path' | 'managed'

export interface ServerCandidate {
  source: Exclude<InstallSource, 'build'>
  binary: string
  version?: string | null
  app?: string
}

export interface Detection {
  servers: ServerCandidate[]
  apps: { path: string; version?: string | null; has_server: boolean }[]
  database: { path: string; exists: boolean; size_bytes?: number | null; projects?: number | null; tasks?: number | null }
}

export interface ExtensionStatus {
  id: 'projecthub'
  name: string
  description: string
  homepage: string
  state: ExtensionState
  installed: boolean
  enabled: boolean
  running: boolean
  url?: string | null
  pid?: number | null
  started_at?: string | null
  error?: string | null
  config: ProjectHubConfig
  self_test_ok: boolean
  install_root: string
  install: {
    running: boolean
    phase: string
    started_at?: string | null
    finished_at?: string | null
    error?: string | null
    log: string[]
  }
  detected: Detection
}

const EXT = '/api/extensions/projecthub'

export const extensions = {
  list: () => request<ExtensionStatus[]>('/api/extensions'),
  status: () => request<ExtensionStatus>(EXT),
  install: (body: { repo?: string; branch?: string } = {}) =>
    request<ExtensionStatus>(`${EXT}/install`, { method: 'POST', body: JSON.stringify(body) }),
  detect: () => request<ExtensionStatus>(`${EXT}/detect`, { method: 'POST' }),
  useInstalled: (binary?: string) =>
    request<ExtensionStatus>(`${EXT}/use-installed`, { method: 'POST', body: JSON.stringify({ binary }) }),
  selfTest: () =>
    request<{ report: SelfTestReport; status: ExtensionStatus }>(`${EXT}/self-test`, { method: 'POST' }),
  enable: () => request<ExtensionStatus>(`${EXT}/enable`, { method: 'POST' }),
  disable: () => request<ExtensionStatus>(`${EXT}/disable`, { method: 'POST' }),
  restart: () => request<ExtensionStatus>(`${EXT}/restart`, { method: 'POST' }),
  config: (body: Partial<Pick<ProjectHubConfig, 'commands' | 'repo' | 'branch'>> & { db_path?: string }) =>
    request<ExtensionStatus>(`${EXT}/config`, { method: 'PUT', body: JSON.stringify(body) }),
  uninstall: () => request<ExtensionStatus>(`${EXT}/uninstall`, { method: 'POST' }),
}

// ─── ProjectHub data (proxied to project-hub-server /api/v1) ────────────────

export interface PhProject {
  id: string
  name: string
  root_path: string
  language: string
  project_type: string
  status: string
  description?: string | null
  group_name?: string | null
  last_modified_at?: string | null
  has_git?: boolean
  git_dirty?: boolean
  depth?: number
}

export type PhTaskStatus = 'todo' | 'in_progress' | 'done' | 'cancelled'

export interface PhTaskLink {
  github_repo?: string | null
  github_issue_number?: number | null
  github_issue_url?: string | null
  github_issue_state?: string | null
  source?: string | null
  external_ref?: string | null
}

export interface PhTask {
  id: string
  project_id?: string | null
  title: string
  description?: string | null
  status: PhTaskStatus
  priority: 'low' | 'medium' | 'high' | 'critical'
  due_at?: string | null
  created_at: string
  updated_at: string
  link?: PhTaskLink | null
}

export interface PhCommit {
  hash: string
  message: string
  author: string
  date: string
}

export interface PhDetail {
  project: PhProject
  exists: boolean
  packages: string[]
  commands: { name: string; command: string }[]
  git: {
    status: { branch: string; modified: number; staged: number; untracked: number; ahead: number; behind: number }
    recent_commits: PhCommit[]
  } | null
  brdd: {
    exists: boolean
    version?: string | null
    stack: string[]
    analyzed_at?: string | null
    snapshots: number
    has_notes: boolean
  }
  github_repo?: string | null
  tasks: PhTask[]
}

export interface PhBrdd {
  exists: boolean
  dir: string
  analysis: {
    version?: string | null
    stack: string[]
    analyzed_at: string
    structure: { files: number; loc: number }
  } | null
  versions: { n: number; at: string; version?: string | null; reason: string; commit_short?: string | null }[]
  summary_md?: string | null
  changelog_md?: string | null
  notes_md?: string | null
}

const PH = '/api/projecthub'
const qs = (params: Record<string, string | number | undefined | null>) => {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
  const s = q.toString()
  return s ? `?${s}` : ''
}

export const ph = {
  health: () => request<Record<string, unknown>>(`${PH}/health`),
  projects: (params: { q?: string; limit?: number; offset?: number } = {}) =>
    request<{ total: number; items: PhProject[] }>(`${PH}/projects${qs(params)}`),
  detail: (id: string) => request<PhDetail>(`${PH}/projects/${id}/detail`),
  scan: (path: string) => request<{ projects: PhProject[] }>(`${PH}/roots`, { method: 'POST', body: JSON.stringify({ path }) }),
  brdd: (id: string) => request<PhBrdd>(`${PH}/projects/${id}/brdd`),
  brddRefresh: (id: string, force_snapshot = false) =>
    request<unknown>(`${PH}/projects/${id}/brdd/refresh`, { method: 'POST', body: JSON.stringify({ force_snapshot }) }),
  brddRefreshAll: () => request<{ refreshed: number; failed: number }>(`${PH}/brdd/refresh-all`, { method: 'POST', body: '{}' }),
  tasks: (params: { project_id?: string; status?: string; source?: string; ids?: string } = {}) =>
    request<PhTask[]>(`${PH}/tasks${qs(params)}`),
  createTask: (body: {
    project_id?: string | null
    title: string
    description?: string
    priority?: string
    due_at?: string
    source?: string
    external_ref?: string
  }) => request<PhTask>(`${PH}/tasks`, { method: 'POST', body: JSON.stringify({ source: 'boarddo', ...body }) }),
  updateTask: (id: string, body: Partial<Pick<PhTask, 'status' | 'title' | 'description' | 'priority'>>) =>
    request<PhTask>(`${PH}/tasks/${id}`, { method: 'PATCH', body: JSON.stringify(body) }),
  deleteTask: (id: string) => request<void>(`${PH}/tasks/${id}`, { method: 'DELETE' }),
  createIssue: (id: string, labels: string[] = ['boarddo']) =>
    request<PhTask>(`${PH}/tasks/${id}/github-issue`, { method: 'POST', body: JSON.stringify({ labels }) }),
  syncIssues: () =>
    request<{ checked: number; tasks_completed: number; tasks_reopened: number; issues_closed: number; errors: string[] }>(
      `${PH}/github/issues/sync`,
      { method: 'POST', body: '{}' },
    ),
  github: () => request<{ config: { connected: boolean; username?: string | null }; linked_issues: number }>(`${PH}/github`),
  runCommand: (id: string, command: string) =>
    request<{ command: string; exit_code: number; ok: boolean; duration_ms: number; stdout: string; stderr: string }>(
      `${PH}/projects/${id}/commands/run`,
      { method: 'POST', body: JSON.stringify({ command }) },
    ),
}

export const phAi = {
  analyze: (id: string, locale?: string) =>
    request<{ model: string; bundle: PhBrdd }>(`/api/projecthub-ai/projects/${id}/analyze`, {
      method: 'POST',
      body: JSON.stringify({ locale }),
    }),
  suggestTasks: (
    id: string,
    body: { instruction?: string; count?: number; apply?: boolean; github_issues?: boolean; locale?: string },
  ) =>
    request<{
      model: string
      tasks: { title: string; description?: string; priority?: string; due_in_days?: number | null }[]
      created: PhTask[]
      errors: string[]
    }>(`/api/projecthub-ai/projects/${id}/suggest-tasks`, { method: 'POST', body: JSON.stringify(body) }),
}
