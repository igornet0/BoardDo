import { TYPE_IDS, type NodeCategory } from '../types'
import type { MessageKey } from '../i18n/messages'

export interface PaletteItem {
  typeId: string
  labelKey: MessageKey
  category: NodeCategory
  groupKey: MessageKey
  defaultConfig: Record<string, unknown>
  /** Shown only when this extension is enabled. */
  requires?: 'projecthub'
}

/** Palette entries the current extension set allows. */
export function availablePalette(items: PaletteItem[], projecthubReady: boolean): PaletteItem[] {
  return items.filter((i) => !i.requires || (i.requires === 'projecthub' && projecthubReady))
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
    typeId: TYPE_IDS.ALERTWIM_ALERT,
    labelKey: 'palette.alertwim.alert',
    category: 'action',
    groupKey: 'palette.group.alertwim',
    defaultConfig: { level: 'info', title: '', message: '{{message}}', source: '', show: true, launch: true },
  },
  {
    typeId: TYPE_IDS.PH_PROJECTS,
    labelKey: 'palette.ph.projects',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { query: '', limit: 50 },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_PROJECT,
    labelKey: 'palette.ph.project',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '' },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_BRDD,
    labelKey: 'palette.ph.brdd',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '', action: 'read' },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_CODE,
    labelKey: 'palette.ph.code',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '', action: 'tree', path: '', depth: 2 },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_GIT,
    labelKey: 'palette.ph.git',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '', limit: 20 },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_TASKS,
    labelKey: 'palette.ph.tasks',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '', status: '', source: '' },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_TASK_CREATE,
    labelKey: 'palette.ph.taskCreate',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '', title: '{{trigger.text}}', description: '', priority: 'medium', github_issue: false },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_TASK_UPDATE,
    labelKey: 'palette.ph.taskUpdate',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { task_id: '', status: 'done' },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_COMMAND,
    labelKey: 'palette.ph.command',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { project: '', command: '', timeout_secs: 300, fail_on_error: true },
    requires: 'projecthub',
  },
  {
    typeId: TYPE_IDS.PH_GITHUB,
    labelKey: 'palette.ph.github',
    category: 'action',
    groupKey: 'palette.group.projecthub',
    defaultConfig: { action: 'sync' },
    requires: 'projecthub',
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
      read_pages: 3,
      page_chars: 3000,
      freshness: 'auto',
      render: 'auto',
    },
  },
  {
    typeId: TYPE_IDS.WEB_OPEN,
    labelKey: 'palette.action.webOpen',
    category: 'action',
    groupKey: 'palette.group.actions',
    defaultConfig: {
      url: '{{nodes.search.output.results}}',
      limit: 3,
      max_chars: 8000,
      selector: '',
      fields: '',
      include_links: true,
      render: 'auto',
      wait_for: '',
      script: '',
      screenshot: false,
      structured: true,
      capture_network: false,
    },
  },
  {
    typeId: TYPE_IDS.WEB_EXTRACT,
    labelKey: 'palette.action.webExtract',
    category: 'action',
    groupKey: 'palette.group.ai',
    defaultConfig: {
      connection_id: '',
      model: 'gpt-4o-mini',
      url: '{{nodes.search.output.results}}',
      instruction: 'Название товара, цена и наличие',
      schema: 'name: название\nprice: цена\navailability: наличие',
      limit: 3,
      render: 'auto',
      capture_network: false,
      strict: false,
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
    typeId: TYPE_IDS.TOOL_RUN,
    labelKey: 'palette.action.tool',
    category: 'action',
    groupKey: 'palette.group.actions',
    defaultConfig: { tool_name: '', input: {}, fail_on_error: true },
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
      reply_to_message_id: '{{trigger.message_id}}',
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
    typeId: TYPE_IDS.TELEGRAM_USER_GET_CHAT_MEMBERS,
    labelKey: 'palette.telegramUser.members',
    category: 'action',
    groupKey: 'palette.group.telegramUser',
    defaultConfig: {
      account_id: '{{trigger.account_id}}',
      chat_id: '{{trigger.chat_id}}',
      limit: 200,
      cache_ttl_hours: 24,
      continue_on_error: true,
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

/** Integrations get their own palette section (in this order) instead of a shared "Actions". */
export const INTEGRATION_GROUPS = [
  'palette.group.alertwim',
  'palette.group.github',
  'palette.group.projecthub',
  'palette.group.telegram',
  'palette.group.telegramUser',
] as const satisfies readonly MessageKey[]

export type IntegrationGroup = (typeof INTEGRATION_GROUPS)[number]

/** The integration section of a node; triggers stay with the other triggers. */
export function integrationGroupFor(item: PaletteItem): IntegrationGroup | null {
  if (item.category === 'trigger') return null
  return (INTEGRATION_GROUPS as readonly MessageKey[]).includes(item.groupKey)
    ? (item.groupKey as IntegrationGroup)
    : null
}

export function labelKeyForType(typeId: string): MessageKey | null {
  return PALETTE.find((p) => p.typeId === typeId)?.labelKey ?? null
}

export function categoryForType(typeId: string): NodeCategory {
  return PALETTE.find((p) => p.typeId === typeId)?.category ?? 'action'
}
