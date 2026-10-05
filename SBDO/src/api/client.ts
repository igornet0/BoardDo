const API_BASE = import.meta.env.VITE_API_BASE ?? ''

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export async function request<T>(
  path: string,
  init?: RequestInit,
): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  })

  if (!res.ok) {
    const text = await res.text()
    throw new ApiError(text || res.statusText, res.status)
  }

  if (res.status === 204) {
    return undefined as T
  }

  return res.json() as Promise<T>
}

export function wsUrl(): string {
  const base = import.meta.env.VITE_WS_URL
  if (base) return base
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${window.location.host}/ws`
}

export function telegramWsUrl(): string {
  const base = import.meta.env.VITE_TELEGRAM_WS_URL
  if (base) return base
  const root = wsUrl()
  if (root.endsWith('/ws')) return `${root}/telegram`
  return `${root.replace(/\/$/, '')}/ws/telegram`
}
