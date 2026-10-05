import { TYPE_IDS, type NodeCategory } from '../types'
import type { MessageKey } from '../i18n/messages'

export interface PaletteItem {
  typeId: string
  labelKey: MessageKey
  category: NodeCategory
  groupKey: MessageKey
  defaultConfig: Record<string, unknown>
}

export const PALETTE: PaletteItem[] = [
  {
    typeId: TYPE_IDS.TRIGGER_MANUAL,
    labelKey: 'palette.trigger.manual',
    category: 'trigger',
    groupKey: 'palette.group.triggers',
    defaultConfig: {},
  },
  {
    typeId: TYPE_IDS.TRIGGER_WEBHOOK,
    labelKey: 'palette.trigger.webhook',
    category: 'trigger',
    groupKey: 'palette.group.triggers',
    defaultConfig: {},
  },
  {
    typeId: TYPE_IDS.TRIGGER_SCHEDULE,
    labelKey: 'palette.trigger.schedule',
    category: 'trigger',
    groupKey: 'palette.group.triggers',
    defaultConfig: {
      every: { minutes: 5 },
      timezone: 'UTC',
      on_overlap: 'skip',
    },
  },
  {
    typeId: TYPE_IDS.LOGIC_CONDITION,
    labelKey: 'palette.logic.condition',
    category: 'condition',
    groupKey: 'palette.group.logic',
    defaultConfig: { expression: '{{amount > 100}}' },
  },
  {
    typeId: TYPE_IDS.LOGIC_DELAY,
    labelKey: 'palette.logic.delay',
    category: 'logic',
    groupKey: 'palette.group.logic',
    defaultConfig: { ms: 500 },
  },
  {
    typeId: TYPE_IDS.DATA_SET,
    labelKey: 'palette.data.set',
    category: 'data',
    groupKey: 'palette.group.data',
    defaultConfig: { name: 'amount', value: '{{trigger.amount}}' },
  },
  {
    typeId: TYPE_IDS.DATA_TRANSFORM,
    labelKey: 'palette.data.transform',
    category: 'data',
    groupKey: 'palette.group.data',
    defaultConfig: {
      mapping: {
        message: 'Hello {{nodes.get_user.output.name}}',
        fee: '{{trigger.amount * 0.2}}',
      },
    },
  },
  {
    typeId: TYPE_IDS.HTTP_REQUEST,
    labelKey: 'palette.action.http',
    category: 'action',
    groupKey: 'palette.group.actions',
    defaultConfig: {
      method: 'GET',
      url: 'https://httpbin.org/get',
      connection_id: '',
      headers: {},
      query: {},
      timeout_ms: 10000,
      retry: { max: 1, backoff_ms: 300 },
    },
  },
  {
    typeId: TYPE_IDS.GITHUB_GET_LATEST_RELEASE,
    labelKey: 'palette.github.latestRelease',
    category: 'action',
    groupKey: 'palette.group.github',
    defaultConfig: {
      connection_id: '',
      owner: '',
      repo: '',
      track_new: true,
    },
  },
  {
    typeId: TYPE_IDS.AI_CHAT,
    labelKey: 'palette.ai.chat',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'gpt-4o-mini',
      system: 'You are a helpful assistant. Answer briefly.',
      prompt: '{{trigger.text}}',
      strip_prefix: '@ai',
      temperature: 0.2,
      max_tokens: 1024,
    },
  },
  {
    typeId: TYPE_IDS.AI_CLASSIFY,
    labelKey: 'palette.ai.classify',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'gpt-4o-mini',
      system: '',
      text: '{{trigger.text}}',
      labels: ['needs_web_search', 'knowledge'],
    },
  },
  {
    typeId: TYPE_IDS.AI_ANALYZE,
    labelKey: 'palette.ai.analyze',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'gpt-4o-mini',
      system: '',
      prompt: '{{trigger.text}}',
    },
  },
  {
    typeId: TYPE_IDS.AI_IMAGE,
    labelKey: 'palette.ai.image',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'gpt-image-1',
      mode: 'generate',
      prompt: '{{trigger.text}}',
      image: '',
      size: '1024x1024',
    },
  },
  {
    typeId: TYPE_IDS.AI_AUDIO,
    labelKey: 'palette.ai.audio',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'gpt-4o-mini-tts',
      mode: 'tts',
      text: '{{trigger.text}}',
      audio: '',
      voice: 'alloy',
      format: 'mp3',
    },
  },
  {
    typeId: TYPE_IDS.AI_VIDEO,
    labelKey: 'palette.ai.video',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'sora-2',
      prompt: '{{trigger.text}}',
      size: '',
      seconds: 4,
    },
  },
  {
    typeId: TYPE_IDS.WEB_SEARCH,
    labelKey: 'palette.action.webSearch',
    category: 'action',
    groupKey: 'palette.group.actions',
    defaultConfig: {
      query: '{{nodes.classify.output.query}}',
      limit: 8,
    },
  },
  {
    typeId: TYPE_IDS.WEB_FETCH,
    labelKey: 'palette.action.webFetch',
    category: 'action',
    groupKey: 'palette.group.actions',
    defaultConfig: {
      url: '{{nodes.search.output.results.0.url}}',
      method: 'GET',
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_SEND_MESSAGE,
    labelKey: 'palette.telegram.message',
    category: 'action',
    groupKey: 'palette.group.telegram',
    defaultConfig: {
      connection_id: '',
      chat_id: '{{variables.chat_id}}',
      text: '{{trigger.message}}',
      parse_mode: 'markdown',
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_SEND_PHOTO,
    labelKey: 'palette.telegram.photo',
    category: 'action',
    groupKey: 'palette.group.telegram',
    defaultConfig: {
      connection_id: '',
      chat_id: '{{variables.chat_id}}',
      photo: '{{trigger.photo}}',
      caption: '{{trigger.caption}}',
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_SEND_DOCUMENT,
    labelKey: 'palette.telegram.document',
    category: 'action',
    groupKey: 'palette.group.telegram',
    defaultConfig: {
      connection_id: '',
      chat_id: '{{variables.chat_id}}',
      document: '{{trigger.document}}',
      caption: '{{trigger.caption}}',
    },
  },
  {
    typeId: TYPE_IDS.DEBUG_LOG,
    labelKey: 'palette.action.log',
    category: 'action',
    groupKey: 'palette.group.actions',
    defaultConfig: { message: '{{message}}' },
  },
  {
    typeId: TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
    labelKey: 'palette.telegramUser.triggerMessage',
    category: 'trigger',
    groupKey: 'palette.group.telegramUser',
    defaultConfig: {
      account_id: 'any',
      text_contains: '',
      only_chat_ids: '',
      ignore_chat_ids: '',
      ignore_outgoing: true,
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE,
    labelKey: 'palette.telegramUser.send',
    category: 'action',
    groupKey: 'palette.group.telegramUser',
    defaultConfig: {
      account_id: '{{trigger.account_id}}',
      chat_id: '{{trigger.chat_id}}',
      text: '{{nodes.ai.output.text}}',
      parse_mode: 'markdown',
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_USER_FORWARD_MESSAGE,
    labelKey: 'palette.telegramUser.forward',
    category: 'action',
    groupKey: 'palette.group.telegramUser',
    defaultConfig: {
      account_id: '',
      from_chat_id: '{{trigger.chat_id}}',
      to_chat_id: '',
      message_id: '{{trigger.message_id}}',
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_USER_EDIT_MESSAGE,
    labelKey: 'palette.telegramUser.edit',
    category: 'action',
    groupKey: 'palette.group.telegramUser',
    defaultConfig: {
      account_id: '',
      chat_id: '{{trigger.chat_id}}',
      message_id: '{{trigger.message_id}}',
      text: '',
      parse_mode: 'markdown',
    },
  },
  {
    typeId: TYPE_IDS.TELEGRAM_USER_DELETE_MESSAGES,
    labelKey: 'palette.telegramUser.delete',
    category: 'action',
    groupKey: 'palette.group.telegramUser',
    defaultConfig: {
      account_id: '',
      chat_id: '{{trigger.chat_id}}',
      message_id: '{{trigger.message_id}}',
    },
  },
]

export function labelKeyForType(typeId: string): MessageKey | null {
  return PALETTE.find((p) => p.typeId === typeId)?.labelKey ?? null
}

export function categoryForType(typeId: string): NodeCategory {
  return PALETTE.find((p) => p.typeId === typeId)?.category ?? 'action'
}
