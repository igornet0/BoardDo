import { TYPE_IDS } from '../types'
import type { ScenarioTemplate } from './types'

/** Built-in scenario library — load onto canvas, then customize & save. */
export const BUILTIN_SCENARIOS: ScenarioTemplate[] = [
  {
    id: 'telegram-ai-reply',
    nameKey: 'scenarios.telegramAi.name',
    descriptionKey: 'scenarios.telegramAi.desc',
    setupHintKey: 'scenarios.telegramAi.setup',
    tags: ['telegram', 'ai'],
    suggestedStatus: 'active',
    definition: {
      nodes: [
        {
          id: 'tg_in',
          type_id: TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
          category: null,
          position: { x: 280, y: 40 },
          config: {
            account_id: 'any',
            text_contains: '@ai',
            ignore_outgoing: true,
          },
        },
        {
          id: 'ai',
          type_id: TYPE_IDS.AI_CHAT,
          category: null,
          position: { x: 280, y: 200 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system:
              'You are a helpful assistant. Answer briefly in the same language as the user. Use Markdown for emphasis when helpful (**bold**, *italic*, `code`).',
            prompt: '{{trigger.text}}',
            strip_prefix: '@ai',
            temperature: 0.2,
            max_tokens: 1024,
          },
        },
        {
          id: 'tg_out',
          type_id: TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE,
          category: null,
          position: { x: 280, y: 380 },
          config: {
            account_id: '{{trigger.account_id}}',
            chat_id: '{{trigger.chat_id}}',
            text: '{{nodes.ai.output.text}}',
            parse_mode: 'markdown',
          },
        },
      ],
      edges: [
        { id: 'e_tg_ai', source: 'tg_in', target: 'ai' },
        { id: 'e_ai_out', source: 'ai', target: 'tg_out' },
      ],
    },
  },
  {
    id: 'telegram-echo',
    nameKey: 'scenarios.telegramEcho.name',
    descriptionKey: 'scenarios.telegramEcho.desc',
    setupHintKey: 'scenarios.telegramEcho.setup',
    tags: ['telegram', 'starter'],
    suggestedStatus: 'active',
    definition: {
      nodes: [
        {
          id: 'tg_in',
          type_id: TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
          category: null,
          position: { x: 280, y: 40 },
          config: {
            account_id: 'any',
            text_contains: '',
            ignore_outgoing: true,
          },
        },
        {
          id: 'tg_out',
          type_id: TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE,
          category: null,
          position: { x: 280, y: 220 },
          config: {
            account_id: '{{trigger.account_id}}',
            chat_id: '{{trigger.chat_id}}',
            text: '{{trigger.text}}',
            parse_mode: 'plain',
          },
        },
      ],
      edges: [{ id: 'e_echo', source: 'tg_in', target: 'tg_out' }],
    },
  },
  {
    id: 'amount-gate',
    nameKey: 'scenarios.amountGate.name',
    descriptionKey: 'scenarios.amountGate.desc',
    setupHintKey: 'scenarios.amountGate.setup',
    tags: ['logic', 'starter'],
    suggestedStatus: 'draft',
    definition: {
      nodes: [
        {
          id: 'manual',
          type_id: TYPE_IDS.TRIGGER_MANUAL,
          category: null,
          position: { x: 280, y: 40 },
          config: {},
        },
        {
          id: 'set_amount',
          type_id: TYPE_IDS.DATA_SET,
          category: null,
          position: { x: 280, y: 160 },
          config: { name: 'amount', value: 120 },
        },
        {
          id: 'gate',
          type_id: TYPE_IDS.LOGIC_CONDITION,
          category: null,
          position: { x: 280, y: 280 },
          config: {
            left: '{{amount}}',
            operator: 'gte',
            right: 100,
          },
        },
        {
          id: 'log_ok',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 120, y: 420 },
          config: { message: 'Amount OK: {{amount}}' },
        },
        {
          id: 'log_low',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 440, y: 420 },
          config: { message: 'Amount too low: {{amount}}' },
        },
      ],
      edges: [
        { id: 'e1', source: 'manual', target: 'set_amount' },
        { id: 'e2', source: 'set_amount', target: 'gate' },
        {
          id: 'e3',
          source: 'gate',
          target: 'log_ok',
          source_port: 'true',
        },
        {
          id: 'e4',
          source: 'gate',
          target: 'log_low',
          source_port: 'false',
        },
      ],
    },
  },
  {
    id: 'webhook-http',
    nameKey: 'scenarios.webhookHttp.name',
    descriptionKey: 'scenarios.webhookHttp.desc',
    setupHintKey: 'scenarios.webhookHttp.setup',
    tags: ['http', 'starter'],
    suggestedStatus: 'active',
    definition: {
      nodes: [
        {
          id: 'hook',
          type_id: TYPE_IDS.TRIGGER_WEBHOOK,
          category: null,
          position: { x: 280, y: 40 },
          config: {},
        },
        {
          id: 'http',
          type_id: TYPE_IDS.HTTP_REQUEST,
          category: null,
          position: { x: 280, y: 180 },
          config: {
            method: 'POST',
            url: 'https://httpbin.org/post',
            connection_id: '',
            headers: { 'Content-Type': 'application/json' },
            body: '{{trigger}}',
            timeout_ms: 10000,
            retry: { max: 1, backoff_ms: 300 },
          },
        },
        {
          id: 'log',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 280, y: 340 },
          config: { message: 'HTTP done: {{nodes.http.output.status}}' },
        },
      ],
      edges: [
        { id: 'e_hook_http', source: 'hook', target: 'http' },
        { id: 'e_http_log', source: 'http', target: 'log' },
      ],
    },
  },
  {
    id: 'market-monitor',
    nameKey: 'scenarios.marketMonitor.name',
    descriptionKey: 'scenarios.marketMonitor.desc',
    setupHintKey: 'scenarios.marketMonitor.setup',
    tags: ['http', 'starter'],
    suggestedStatus: 'active',
    definition: {
      nodes: [
        {
          id: 'tick',
          type_id: TYPE_IDS.TRIGGER_SCHEDULE,
          category: null,
          position: { x: 280, y: 40 },
          config: {
            every: { seconds: 60 },
            timezone: 'UTC',
            on_overlap: 'skip',
          },
        },
        {
          id: 'log',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 280, y: 200 },
          config: { message: 'tick {{trigger.scheduled_at}}' },
        },
      ],
      edges: [{ id: 'e_tick', source: 'tick', target: 'log' }],
    },
  },
  {
    id: 'github-watch-releases',
    nameKey: 'scenarios.githubWatchReleases.name',
    descriptionKey: 'scenarios.githubWatchReleases.desc',
    setupHintKey: 'scenarios.githubWatchReleases.setup',
    tags: ['github', 'starter'],
    suggestedStatus: 'active',
    definition: {
      nodes: [
        {
          id: 'tick',
          type_id: TYPE_IDS.TRIGGER_SCHEDULE,
          category: null,
          position: { x: 280, y: 40 },
          config: {
            every: { minutes: 5 },
            timezone: 'UTC',
            on_overlap: 'skip',
          },
        },
        {
          id: 'release',
          type_id: TYPE_IDS.GITHUB_GET_LATEST_RELEASE,
          category: null,
          position: { x: 280, y: 200 },
          config: {
            connection_id: '',
            owner: '',
            repo: '',
            track_new: true,
          },
        },
        {
          id: 'gate',
          type_id: TYPE_IDS.LOGIC_CONDITION,
          category: null,
          position: { x: 280, y: 360 },
          config: {
            expression: '{{nodes.release.output.is_new}}',
          },
        },
        {
          id: 'log',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 280, y: 520 },
          config: {
            message:
              'New release {{nodes.release.output.tag_name}}: {{nodes.release.output.html_url}}',
          },
        },
      ],
      edges: [
        { id: 'e_tick_rel', source: 'tick', target: 'release' },
        { id: 'e_rel_gate', source: 'release', target: 'gate' },
        {
          id: 'e_gate_log',
          source: 'gate',
          target: 'log',
          source_port: 'true',
        },
      ],
    },
  },
  {
    id: 'ai-manual-chat',
    nameKey: 'scenarios.aiManual.name',
    descriptionKey: 'scenarios.aiManual.desc',
    setupHintKey: 'scenarios.aiManual.setup',
    tags: ['ai', 'starter'],
    suggestedStatus: 'draft',
    definition: {
      nodes: [
        {
          id: 'manual',
          type_id: TYPE_IDS.TRIGGER_MANUAL,
          category: null,
          position: { x: 280, y: 40 },
          config: {},
        },
        {
          id: 'ai',
          type_id: TYPE_IDS.AI_CHAT,
          category: null,
          position: { x: 280, y: 180 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system: 'You are a helpful assistant.',
            prompt: 'Say hello in one short sentence.',
            strip_prefix: '',
            temperature: 0.3,
            max_tokens: 256,
          },
        },
        {
          id: 'log',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 280, y: 340 },
          config: { message: '{{nodes.ai.output.text}}' },
        },
      ],
      edges: [
        { id: 'e_m_ai', source: 'manual', target: 'ai' },
        { id: 'e_ai_log', source: 'ai', target: 'log' },
      ],
    },
  },
  {
    id: 'query-classify-research',
    nameKey: 'scenarios.classifyResearch.name',
    descriptionKey: 'scenarios.classifyResearch.desc',
    setupHintKey: 'scenarios.classifyResearch.setup',
    tags: ['ai', 'starter'],
    suggestedStatus: 'draft',
    definition: {
      nodes: [
        {
          id: 'manual',
          type_id: TYPE_IDS.TRIGGER_MANUAL,
          category: null,
          position: { x: 280, y: 24 },
          config: {},
        },
        {
          id: 'classify',
          type_id: TYPE_IDS.AI_CLASSIFY,
          category: null,
          position: { x: 280, y: 160 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system: '',
            text: '{{trigger.text}}',
            labels: ['needs_web_search', 'knowledge'],
          },
        },
        {
          id: 'gate',
          type_id: TYPE_IDS.LOGIC_CONDITION,
          category: null,
          position: { x: 280, y: 300 },
          config: {
            expression: '{{nodes.classify.output.needs_web_search}}',
          },
        },
        {
          id: 'search',
          type_id: TYPE_IDS.WEB_SEARCH,
          category: null,
          position: { x: 80, y: 440 },
          config: {
            query: '{{nodes.classify.output.query}}',
            limit: 8,
            read_pages: 3,
            page_chars: 3000,
            freshness: 'auto',
          },
        },
        {
          id: 'answer_web',
          type_id: TYPE_IDS.AI_CHAT,
          category: null,
          position: { x: 80, y: 580 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system:
              'Answer using the page texts from the sources. Cite URLs when you use them. If sources are thin, say so. The sources start with today\'s date and the search period: state today\'s date, give the publication date of every news item, and skip items published outside the period.',
            prompt:
              'Question: {{trigger.text}}\n\nSources (page text):\n{{nodes.search.output.context}}',
            strip_prefix: '',
            temperature: 0.2,
            max_tokens: 1024,
          },
        },
        {
          id: 'answer_direct',
          type_id: TYPE_IDS.AI_CHAT,
          category: null,
          position: { x: 480, y: 440 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system:
              'Answer from your knowledge. If the question needs live facts, say you are not searching the web.',
            prompt: '{{trigger.text}}',
            strip_prefix: '',
            temperature: 0.2,
            max_tokens: 1024,
          },
        },
        {
          id: 'log',
          type_id: TYPE_IDS.DEBUG_LOG,
          category: null,
          position: { x: 280, y: 720 },
          config: {
            message:
              '{{nodes.answer_web.output.text}}{{nodes.answer_direct.output.text}}',
          },
        },
      ],
      edges: [
        { id: 'e_m_c', source: 'manual', target: 'classify' },
        { id: 'e_c_g', source: 'classify', target: 'gate' },
        {
          id: 'e_g_s',
          source: 'gate',
          target: 'search',
          source_port: 'true',
        },
        { id: 'e_s_a', source: 'search', target: 'answer_web' },
        { id: 'e_a_l', source: 'answer_web', target: 'log' },
        {
          id: 'e_g_d',
          source: 'gate',
          target: 'answer_direct',
          source_port: 'false',
        },
        { id: 'e_d_l', source: 'answer_direct', target: 'log' },
      ],
    },
  },
  {
    id: 'telegram-classify-research',
    nameKey: 'scenarios.telegramClassifyResearch.name',
    descriptionKey: 'scenarios.telegramClassifyResearch.desc',
    setupHintKey: 'scenarios.telegramClassifyResearch.setup',
    tags: ['telegram', 'ai'],
    suggestedStatus: 'active',
    definition: {
      nodes: [
        {
          id: 'incoming',
          type_id: TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED,
          category: null,
          position: { x: 280, y: 24 },
          config: {
            account_id: 'any',
            text_contains: '',
            ignore_outgoing: true,
          },
        },
        {
          id: 'classify',
          type_id: TYPE_IDS.AI_CLASSIFY,
          category: null,
          position: { x: 280, y: 160 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system: '',
            text: '{{trigger.text}}',
            labels: ['needs_web_search', 'knowledge'],
          },
        },
        {
          id: 'gate',
          type_id: TYPE_IDS.LOGIC_CONDITION,
          category: null,
          position: { x: 280, y: 300 },
          config: {
            expression: '{{nodes.classify.output.needs_web_search}}',
          },
        },
        {
          id: 'search',
          type_id: TYPE_IDS.WEB_SEARCH,
          category: null,
          position: { x: 80, y: 440 },
          config: {
            query: '{{nodes.classify.output.query}}',
            limit: 8,
            read_pages: 3,
            page_chars: 3000,
            freshness: 'auto',
          },
        },
        {
          id: 'answer_web',
          type_id: TYPE_IDS.AI_CHAT,
          category: null,
          position: { x: 80, y: 580 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system:
              'Answer using the page texts from the sources. Cite URLs when you use them. Keep it short for Telegram. The sources start with today\'s date and the search period: state today\'s date, give the publication date of every news item, and skip items published outside the period.',
            prompt:
              'Question: {{trigger.text}}\n\nSources (page text):\n{{nodes.search.output.context}}',
            strip_prefix: '',
            temperature: 0.2,
            max_tokens: 800,
          },
        },
        {
          id: 'answer_direct',
          type_id: TYPE_IDS.AI_CHAT,
          category: null,
          position: { x: 480, y: 440 },
          config: {
            connection_id: '',
            model: 'gpt-4o-mini',
            system: 'Answer briefly from your knowledge for Telegram.',
            prompt: '{{trigger.text}}',
            strip_prefix: '',
            temperature: 0.2,
            max_tokens: 800,
          },
        },
        {
          id: 'reply',
          type_id: TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE,
          category: null,
          position: { x: 280, y: 720 },
          config: {
            account_id: '{{trigger.account_id}}',
            chat_id: '{{trigger.chat_id}}',
            text: '{{nodes.answer_web.output.text}}{{nodes.answer_direct.output.text}}',
            parse_mode: 'markdown',
          },
        },
      ],
      edges: [
        { id: 'e_i_c', source: 'incoming', target: 'classify' },
        { id: 'e_c_g', source: 'classify', target: 'gate' },
        {
          id: 'e_g_s',
          source: 'gate',
          target: 'search',
          source_port: 'true',
        },
        { id: 'e_s_a', source: 'search', target: 'answer_web' },
        { id: 'e_a_r', source: 'answer_web', target: 'reply' },
        {
          id: 'e_g_d',
          source: 'gate',
          target: 'answer_direct',
          source_port: 'false',
        },
        { id: 'e_d_r', source: 'answer_direct', target: 'reply' },
      ],
    },
  },
]

export function getScenarioById(id: string): ScenarioTemplate | undefined {
  return BUILTIN_SCENARIOS.find((s) => s.id === id)
}
