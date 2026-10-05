import { request, telegramWsUrl } from './client'

export type TelegramAccountStatus =
  | 'created'
  | 'initializing'
  | 'wait_phone_number'
  | 'wait_code'
  | 'wait_password'
  | 'wait_registration'
  | 'ready'
  | 'closing'
  | 'disconnected'
  | 'error'

export interface TelegramAccountPermissions {
  read_messages: boolean
  send_messages: boolean
  forward_messages: boolean
  edit_messages: boolean
  delete_messages: boolean
  manage_chats: boolean
}

export interface TelegramAccount {
  id: string
  status: TelegramAccountStatus
  phone_masked?: string | null
  username?: string | null
  display_name?: string | null
  permissions: TelegramAccountPermissions
  desired_running: boolean
  error?: string | null
  created_at: string
  updated_at: string
}

export interface TelegramAccountCreated {
  account: TelegramAccount
  consent: string
}

export interface TelegramChat {
  id: number
  title: string
  chat_type: string
  username?: string | null
}

export interface TelegramMessage {
  chat_id: number
  message_id: number
  sender_id?: number | null
  text?: string | null
  timestamp: string
}

export interface TelegramCapabilities {
  mock_simulate: boolean
  tdlib: boolean
}

export interface TelegramAuditEvent {
  id: string
  account_id: string
  scenario_id?: string | null
  execution_id?: string | null
  event_type: string
  action: string
  target?: string | null
  status: 'success' | 'failed'
  error?: string | null
  created_at: string
}

export function listTelegramAccounts() {
  return request<TelegramAccount[]>('/api/telegram/accounts')
}

export function createTelegramAccount() {
  return request<TelegramAccountCreated>('/api/telegram/accounts', {
    method: 'POST',
    body: JSON.stringify({}),
  })
}

export function getTelegramAccount(id: string) {
  return request<TelegramAccount>(`/api/telegram/accounts/${id}`)
}

export function deleteTelegramAccount(id: string) {
  return request<void>(`/api/telegram/accounts/${id}`, { method: 'DELETE' })
}

export function connectTelegramAccount(id: string) {
  return request<void>(`/api/telegram/accounts/${id}/connect`, { method: 'POST' })
}

export function disconnectTelegramAccount(id: string) {
  return request<void>(`/api/telegram/accounts/${id}/disconnect`, {
    method: 'POST',
  })
}

export function submitTelegramPhone(id: string, phone: string) {
  return request<void>(`/api/telegram/accounts/${id}/auth/phone`, {
    method: 'POST',
    body: JSON.stringify({ phone }),
  })
}

export function submitTelegramCode(id: string, code: string) {
  return request<void>(`/api/telegram/accounts/${id}/auth/code`, {
    method: 'POST',
    body: JSON.stringify({ code }),
  })
}

export function submitTelegramPassword(id: string, password: string) {
  return request<void>(`/api/telegram/accounts/${id}/auth/password`, {
    method: 'POST',
    body: JSON.stringify({ password }),
  })
}

export function listTelegramChats(id: string) {
  return request<{ chats: TelegramChat[] }>(`/api/telegram/accounts/${id}/chats`)
}

export function searchTelegramChats(id: string, query: string) {
  const q = encodeURIComponent(query)
  return request<{ chats: TelegramChat[] }>(
    `/api/telegram/accounts/${id}/chats/search?q=${q}&limit=20`,
  )
}

export function listTelegramMessages(id: string, chatId: number) {
  return request<TelegramMessage[]>(
    `/api/telegram/accounts/${id}/messages?chat_id=${chatId}`,
  )
}

export function sendTelegramUserMessage(
  id: string,
  chatId: number,
  text: string,
) {
  return request<{ message_id: number }>(
    `/api/telegram/accounts/${id}/messages/send`,
    {
      method: 'POST',
      body: JSON.stringify({ chat_id: chatId, text }),
    },
  )
}

export function getTelegramCapabilities() {
  return request<TelegramCapabilities>('/api/telegram/capabilities')
}

export function simulateTelegramMessage(
  id: string,
  chatId: number,
  messageId: number,
  text: string,
) {
  return request<void>(`/api/telegram/accounts/${id}/simulate/message`, {
    method: 'POST',
    body: JSON.stringify({ chat_id: chatId, message_id: messageId, text }),
  })
}

export function listTelegramAudit(accountId?: string) {
  const q = accountId ? `?account_id=${accountId}` : ''
  return request<TelegramAuditEvent[]>(`/api/telegram/audit${q}`)
}

export interface TelegramAuthStateChanged {
  type: 'auth_state_changed'
  account_id: string
  state: TelegramAccountStatus
  error?: string
}

export type TelegramWsMessage =
  | TelegramAuthStateChanged
  | { type: string; [key: string]: unknown }

export function subscribeTelegramSocket(
  onEvent: (event: TelegramWsMessage) => void,
): () => void {
  let stopped = false
  let socket: WebSocket | null = null
  let retry: number | undefined

  function connect() {
    socket = new WebSocket(telegramWsUrl())
    socket.onmessage = (ev) => {
      try {
        onEvent(JSON.parse(String(ev.data)) as TelegramWsMessage)
      } catch {
        /* ignore malformed */
      }
    }
    socket.onclose = () => {
      if (!stopped) {
        retry = window.setTimeout(connect, 1500)
      }
    }
  }

  connect()
  return () => {
    stopped = true
    if (retry) window.clearTimeout(retry)
    socket?.close()
  }
}
