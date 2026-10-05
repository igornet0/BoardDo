export type ExecutionStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'

export type WorkflowStatus = 'draft' | 'active' | 'archived'

export type NodeCategory =
  | 'trigger'
  | 'action'
  | 'condition'
  | 'transform'
  | 'logic'
  | 'data'

export interface Position {
  x: number
  y: number
}

export interface WorkflowNode {
  id: string
  type_id: string
  category?: NodeCategory | null
  position: Position
  config: Record<string, unknown>
}

export interface WorkflowEdge {
  id: string
  source: string
  target: string
  source_port?: string | null
  target_port?: string | null
}

export interface WorkflowDefinition {
  nodes: WorkflowNode[]
  edges: WorkflowEdge[]
}

export interface WorkflowRecord {
  id: string
  name: string
  description: string
  version: number
  status: WorkflowStatus
  definition: WorkflowDefinition
  created_at: string
  updated_at: string
}

export interface WorkflowSummary {
  id: string
  name: string
  description: string
  version: number
  status: WorkflowStatus
  created_at: string
  updated_at: string
}

export interface CreateWorkflowRequest {
  name: string
  description?: string
  nodes?: WorkflowNode[]
  edges?: WorkflowEdge[]
}

export interface UpdateWorkflowRequest {
  name: string
  description?: string
  status?: WorkflowStatus
  nodes: WorkflowNode[]
  edges: WorkflowEdge[]
}

export interface ValidateResponse {
  valid: boolean
  errors: string[]
}

export interface RunWorkflowResponse {
  execution_id: string
  status: ExecutionStatus
}

export type RuntimeState =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'waiting'
  | 'executing'
  | 'stopping'

export interface RuntimeTriggerInfo {
  kind: string
  label: string
  account_hint?: string | null
}

export interface RuntimeSnapshot {
  workflow_id: string
  workflow_name: string
  state: RuntimeState
  triggers: RuntimeTriggerInfo[]
  executions_total: number
  in_flight: number
  last_event_at?: string | null
  last_event_source?: string | null
  last_execution_id?: string | null
  last_execution_status?: string | null
  next_run_at?: string | null
  started_at?: string | null
}

export interface Connection {
  id: string
  name: string
  type: string
  config: Record<string, unknown>
  enabled: boolean
  has_secret: boolean
  created_at: string
  updated_at: string
}

export interface CreateConnectionRequest {
  name: string
  type: string
  config?: Record<string, unknown>
  secret?: Record<string, unknown>
  enabled?: boolean
}

export interface UpdateConnectionRequest {
  name: string
  type: string
  config?: Record<string, unknown>
  secret?: Record<string, unknown>
  enabled: boolean
}

export interface Execution {
  id: string
  workflow_id: string
  workflow_version: number
  status: ExecutionStatus
  trigger_type?: string
  trigger: unknown
  started_at: string
  finished_at?: string | null
  error?: string | null
  changelog?: ExecutionChangelogEntry[]
}

export interface ExecutionChangelogEntry {
  at: string
  kind: string
  node_id?: string | null
  message?: string | null
  status?: ExecutionStatus | null
  error?: string | null
  duration_ms?: number | null
  output?: unknown
}

export interface ExecutionChangelog {
  execution_id: string
  workflow_id: string
  trigger_type: string
  trigger: unknown
  status: ExecutionStatus
  error?: string | null
  entries: ExecutionChangelogEntry[]
  started_at: string
  finished_at?: string | null
}

export interface NodeExecution {
  id: string
  execution_id: string
  node_id: string
  status: ExecutionStatus
  input: unknown
  output?: unknown
  error?: string | null
  started_at: string
  finished_at?: string | null
  duration_ms?: number | null
}

export type ExecutionEvent =
  | { type: 'execution.started'; execution_id: string; workflow_id: string }
  | { type: 'node.started'; execution_id: string; node_id: string }
  | {
      type: 'node.completed'
      execution_id: string
      node_id: string
      output: unknown
      duration_ms: number
    }
  | { type: 'node.failed'; execution_id: string; node_id: string; error: string }
  | {
      type: 'execution.completed'
      execution_id: string
      status: ExecutionStatus
      error?: string | null
    }

export const TYPE_IDS = {
  TRIGGER_MANUAL: 'trigger.manual',
  TRIGGER_WEBHOOK: 'trigger.webhook',
  TRIGGER_SCHEDULE: 'trigger.schedule',
  DATA_SET: 'data.set',
  DATA_TRANSFORM: 'data.transform',
  LOGIC_CONDITION: 'logic.condition',
  LOGIC_DELAY: 'logic.delay',
  DEBUG_LOG: 'debug.log',
  HTTP_REQUEST: 'http.request',
  TELEGRAM_SEND_MESSAGE: 'telegram.send_message',
  TELEGRAM_SEND_PHOTO: 'telegram.send_photo',
  TELEGRAM_SEND_DOCUMENT: 'telegram.send_document',
  TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED:
    'trigger.telegram.user.message_received',
  TELEGRAM_USER_SEND_MESSAGE: 'telegram.user.send_message',
  TELEGRAM_USER_FORWARD_MESSAGE: 'telegram.user.forward_message',
  TELEGRAM_USER_EDIT_MESSAGE: 'telegram.user.edit_message',
  TELEGRAM_USER_DELETE_MESSAGES: 'telegram.user.delete_messages',
  GITHUB_GET_LATEST_RELEASE: 'github.get_latest_release',
  AI_CHAT: 'ai.chat',
  AI_CLASSIFY: 'ai.classify',
  AI_ANALYZE: 'ai.analyze',
  AI_IMAGE: 'ai.image',
  AI_AUDIO: 'ai.audio',
  AI_VIDEO: 'ai.video',
  WEB_SEARCH: 'web.search',
  WEB_FETCH: 'web.fetch',
} as const

// ── Channel / Stream / Trigger (configuration lifecycle) ───────────────

export type ChannelKind = 'internal' | 'webhook' | 'http' | 'telegram'
export type StreamDirection = 'inbound' | 'outbound'
export type TriggerKind = 'event' | 'webhook' | 'schedule' | 'manual'

export interface Channel {
  id: string
  name: string
  description: string
  kind: ChannelKind
  enabled: boolean
  config: Record<string, unknown>
  created_at: string
  updated_at: string
}

export interface Stream {
  id: string
  channel_id: string
  name: string
  description: string
  direction: StreamDirection
  enabled: boolean
  config: Record<string, unknown>
  created_at: string
  updated_at: string
}

export interface Trigger {
  id: string
  stream_id: string
  name: string
  description: string
  kind: TriggerKind
  enabled: boolean
  workflow_id?: string | null
  config: Record<string, unknown>
  created_at: string
  updated_at: string
}

export interface CreateChannelRequest {
  name: string
  description?: string
  kind?: ChannelKind
  enabled?: boolean
  config?: Record<string, unknown>
}

export interface UpdateChannelRequest {
  name: string
  description?: string
  kind: ChannelKind
  enabled: boolean
  config?: Record<string, unknown>
}

export interface CreateStreamRequest {
  channel_id: string
  name: string
  description?: string
  direction?: StreamDirection
  enabled?: boolean
  config?: Record<string, unknown>
}

export interface UpdateStreamRequest {
  channel_id: string
  name: string
  description?: string
  direction: StreamDirection
  enabled: boolean
  config?: Record<string, unknown>
}

export interface CreateTriggerRequest {
  stream_id: string
  name: string
  description?: string
  kind?: TriggerKind
  enabled?: boolean
  workflow_id?: string | null
  config?: Record<string, unknown>
}

export interface UpdateTriggerRequest {
  stream_id: string
  name: string
  description?: string
  kind: TriggerKind
  enabled: boolean
  workflow_id?: string | null
  config?: Record<string, unknown>
}

export type GoalRunStatus =
  | 'draft'
  | 'running'
  | 'paused'
  | 'waiting_approval'
  | 'succeeded'
  | 'failed'
  | 'stopped'

export interface PolicyBundle {
  max_actions_per_day: number
  budget_usd: number
  max_daily_spend_usd?: number
  require_approval: string[]
  allowed_domains: string[]
  blocked_domains: string[]
  kill_switch: boolean
}

export interface AgentTool {
  type_id: string
  title?: string | null
  config?: Record<string, unknown>
}

export interface GoalSpec {
  id: string
  title: string
  text: string
  metric: string
  target: number
  deadline?: string | null
  constraints: Record<string, unknown>
  policy: PolicyBundle
  agent_type_id: string
  instructions: string
  tools: AgentTool[]
  tick_interval_secs: number
  openai_connection_id?: string | null
  created_at: string
  updated_at: string
}

export interface GoalRun {
  id: string
  goal_id: string
  status: GoalRunStatus
  current: number
  target: number
  strategy: Record<string, number | unknown>
  actions_today: number
  actions_day: string
  tick_count: number
  last_tick_at?: string | null
  next_tick_at?: string | null
  last_observe?: string | null
  last_think?: string | null
  error?: string | null
  started_at: string
  finished_at?: string | null
}

export interface GoalDetail {
  spec: GoalSpec
  run?: GoalRun | null
  linked_workflows?: WorkflowSummary[]
}

export interface Experiment {
  id: string
  run_id: string
  number: number
  hypothesis: string
  metrics: Record<string, unknown>
  decision: 'pending' | 'continue' | 'pivot' | 'stop'
  created_at: string
  updated_at: string
}

export interface AgentAuditEntry {
  id: string
  run_id: string
  at: string
  kind: string
  message?: string | null
}

export interface AgentMemoryEntry {
  id: string
  run_id: string
  key: string
  value: unknown
  kind: string
}

export interface AgentApproval {
  id: string
  run_id: string
  goal_id: string
  title: string
  description?: string | null
  capability: string
  tool_type_id: string
  payload: unknown
  status: 'pending' | 'approved' | 'rejected' | 'timed_out' | 'edited'
  created_at: string
}

export interface AgentTemplate {
  id: string
  title: string
  description: string
  agent_type_id: string
  metric: string
  target: number
  instructions: string
  tools: AgentTool[]
  policy: PolicyBundle
}

export interface CreateGoalRequest {
  title: string
  text: string
  metric?: string
  target?: number
  deadline?: string | null
  template_id?: string
  agent_type_id?: string
  tick_interval_secs?: number
  constraints?: Record<string, unknown>
}

export interface MarketingFunnel {
  research_notes: number
  audience_notes: number
  pain_notes: number
  hypotheses: number
  drafts: number
  pending_approval: number
  published: number
  conversations: number
  leads: number
  interested: number
  qualified: number
  conversions: number
}

export interface MarketingLead {
  id: string
  stage: string
  score: number
  score_reasons: string[]
  experiment_id?: string | null
  simulated: boolean
  chat_id?: string | null
  source: string
}

export interface MarketingDraft {
  id: string
  body: string
  status: string
  experiment_id?: string | null
  simulated: boolean
  telegram_message_id?: string | null
}

export interface StrategyDelta {
  experiment_id?: string | null
  observation: string
  action: string
  confidence: number
  created_at: string
}

export interface Learning {
  id: string
  at: string
  observation: string
  decision: string
  reason: string
  confidence: number
  experiment_id?: string | null
}

export interface CampaignPlan {
  summary: string
  competitors: string[]
  segments: string[]
  pain_points: string[]
  content_opportunities: string[]
  acquisition_channels: string[]
  strategy_outline: string
  content_tasks: string[]
  experiments: string[]
  lead_funnels: string[]
  sales_sequences: string[]
  estimated_workload: string
  ready: boolean
  created_at: string
}

export interface CreateCampaignRequest {
  product_name: string
  product_description?: string
  product_url?: string | null
  goal_metric?: string
  target?: number
  budget_usd?: number
  market?: string
  deadline_days?: number
  autonomy?: string
  demo?: boolean
}

export interface CampaignCreatedResponse {
  spec: GoalSpec
  run: GoalRun
  plan: CampaignPlan
}

export interface MarketingSnapshot {
  stage: string
  autonomy_level: number
  autonomy_mode?: string
  funnel: MarketingFunnel
  leads: MarketingLead[]
  drafts: MarketingDraft[]
  research_notes: Array<{
    id: string
    type: string
    content: string
    confidence: number
    source_url?: string | null
  }>
  conversations: Array<{
    id: string
    chat_id: string
    intent: string
    lead_id?: string | null
    simulated: boolean
  }>
  strategy_deltas: StrategyDelta[]
  stage_progress: Record<string, string>
  plan?: CampaignPlan | null
  learnings?: Learning[]
  current_objective?: string | null
  channel_weights?: Record<string, number | unknown>
  plan_ready?: boolean
}

export interface ContentEngineSnapshot {
  project?: {
    id: string
    name: string
    channels: string[]
  } | null
  brand?: { name: string; value_proposition: string } | null
  strategy?: { objectives: string[]; frequency: string } | null
  ideas: Array<{ id: string; title: string; channel: string; format: string }>
  items: Array<{
    id: string
    title: string
    lifecycle: string
    channel: string
    format: string
    parent_content_id?: string | null
  }>
  graph_edges: Array<{ parent_id: string; child_id: string; relation: string }>
  publications: Array<{ content_id: string; channel: string; simulated: boolean }>
  insights: Array<{ observation: string; suggestion: string }>
}
