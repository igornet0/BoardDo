import { request } from './client'

export interface AlertWimStatus {
  id: 'alertwim'
  name: string
  description: string
  repo: string
  installed: boolean
  running: boolean
  url?: string | null
  version?: string | null
  binary?: string | null
  source?: string | null
  connection_id?: string | null
  detected: { path: string; source: string }[]
  install_job: {
    running: boolean
    phase: string
    error?: string | null
    log: string[]
    finished_at?: string | null
  }
}

const EXT = '/api/extensions/alertwim'

export const alertwim = {
  status: () => request<AlertWimStatus>(EXT),
  install: () => request<AlertWimStatus>(`${EXT}/install`, { method: 'POST', body: '{}' }),
  launch: () => request<AlertWimStatus>(`${EXT}/launch`, { method: 'POST', body: '{}' }),
  test: (message?: string) =>
    request<{ delivered: boolean; status: AlertWimStatus }>(`${EXT}/test`, {
      method: 'POST',
      body: JSON.stringify({ message }),
    }),
}
