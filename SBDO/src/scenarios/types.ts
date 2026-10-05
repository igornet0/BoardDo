import type { MessageKey } from '../i18n/messages'
import type { WorkflowDefinition, WorkflowStatus } from '../types'

export type ScenarioTag = 'telegram' | 'ai' | 'http' | 'logic' | 'starter' | 'github'

export interface ScenarioTemplate {
  id: string
  nameKey: MessageKey
  descriptionKey: MessageKey
  /** Shown after load — what the user must configure */
  setupHintKey?: MessageKey
  tags: ScenarioTag[]
  /** Suggested Active for Telegram/schedule triggers */
  suggestedStatus?: WorkflowStatus
  definition: WorkflowDefinition
}
