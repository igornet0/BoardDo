import { request } from './client'
import type {
  Channel,
  CreateChannelRequest,
  CreateStreamRequest,
  CreateTriggerRequest,
  Stream,
  Trigger,
  UpdateChannelRequest,
  UpdateStreamRequest,
  UpdateTriggerRequest,
} from '../types'

export function listChannels() {
  return request<Channel[]>('/api/channels')
}

export function getChannel(id: string) {
  return request<Channel>(`/api/channels/${id}`)
}

export function createChannel(body: CreateChannelRequest) {
  return request<Channel>('/api/channels', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function updateChannel(id: string, body: UpdateChannelRequest) {
  return request<Channel>(`/api/channels/${id}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteChannel(id: string) {
  return request<void>(`/api/channels/${id}`, { method: 'DELETE' })
}

export function listStreams(channelId?: string) {
  const q = channelId ? `?channel_id=${encodeURIComponent(channelId)}` : ''
  return request<Stream[]>(`/api/streams${q}`)
}

export function getStream(id: string) {
  return request<Stream>(`/api/streams/${id}`)
}

export function createStream(body: CreateStreamRequest) {
  return request<Stream>('/api/streams', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function updateStream(id: string, body: UpdateStreamRequest) {
  return request<Stream>(`/api/streams/${id}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteStream(id: string) {
  return request<void>(`/api/streams/${id}`, { method: 'DELETE' })
}

export function listTriggers(streamId?: string) {
  const q = streamId ? `?stream_id=${encodeURIComponent(streamId)}` : ''
  return request<Trigger[]>(`/api/triggers${q}`)
}

export function getTrigger(id: string) {
  return request<Trigger>(`/api/triggers/${id}`)
}

export function createTrigger(body: CreateTriggerRequest) {
  return request<Trigger>('/api/triggers', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function updateTrigger(id: string, body: UpdateTriggerRequest) {
  return request<Trigger>(`/api/triggers/${id}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteTrigger(id: string) {
  return request<void>(`/api/triggers/${id}`, { method: 'DELETE' })
}
