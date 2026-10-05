import type { SVGProps } from 'react'
import { TYPE_IDS } from '../types'

type IconProps = SVGProps<SVGSVGElement> & { size?: number }

function base({ size = 14, ...props }: IconProps) {
  return {
    width: size,
    height: size,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.75,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
    'aria-hidden': true as const,
    ...props,
  }
}

export function IconLightning(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M13 2 4 14h7l-1 8 10-14h-7l1-6z" />
    </svg>
  )
}

export function IconSplit(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M12 3v8" />
      <path d="m12 11-6 8" />
      <path d="m12 11 6 8" />
      <circle cx="12" cy="11" r="1.5" fill="currentColor" stroke="none" />
    </svg>
  )
}

export function IconGlobe(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3a14 14 0 0 1 0 18" />
      <path d="M12 3a14 14 0 0 0 0 18" />
    </svg>
  )
}

export function IconSparkle(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M11 3.5 12.9 9l5.6 2-5.6 2L11 18.5 9.1 13l-5.6-2 5.6-2L11 3.5z" fill="currentColor" fillOpacity={0.18} />
      <path d="M18.5 15.5 19.3 17.7l2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8.8-2.2z" fill="currentColor" stroke="none" />
      <path d="M18.5 3v3M17 4.5h3" />
    </svg>
  )
}

export function IconClock(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </svg>
  )
}

export function IconBraces(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M8 5c-2 0-3 1.5-3 3.5S6 12 6 12s-1 1.5-1 3.5S7 19 8 19" />
      <path d="M16 5c2 0 3 1.5 3 3.5S18 12 18 12s1 1.5 1 3.5S17 19 16 19" />
    </svg>
  )
}

export function IconArrows(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M7 7h10M14 4l3 3-3 3" />
      <path d="M17 17H7M10 14l-3 3 3 3" />
    </svg>
  )
}

export function IconMessage(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v7a2.5 2.5 0 0 1-2.5 2.5H10l-4 3.5V16H6.5A2.5 2.5 0 0 1 4 13.5v-7z" />
    </svg>
  )
}

export function IconLog(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M5 7h14M5 12h10M5 17h12" />
    </svg>
  )
}

export function IconWebhook(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M10 13a3.5 3.5 0 1 0-3 5.5H16" />
      <path d="M14 11a3.5 3.5 0 1 0 3-5.5H8" />
      <path d="M12 7.5v9" />
    </svg>
  )
}

export function IconSearch(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4 4" />
    </svg>
  )
}

export function IconPlay(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M8 5.5v13l11-6.5-11-6.5z" fill="currentColor" stroke="none" />
    </svg>
  )
}

export function IconSave(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M5 4h11l3 3v13H5V4z" />
      <path d="M8 4v5h8V4M8 20v-6h8v6" />
    </svg>
  )
}

export function IconCommand(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M8 8h.01M16 8h.01M8 16h.01M16 16h.01" />
      <path d="M9 8a2.5 2.5 0 1 0-2.5 2.5H12v3H6.5A2.5 2.5 0 1 0 9 16" />
      <path d="M15 8a2.5 2.5 0 1 1 2.5 2.5H12v3h5.5A2.5 2.5 0 1 1 15 16" />
    </svg>
  )
}

export function IconSettings(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M10.3 3.4a1.7 1.7 0 0 1 3.4 0l.1.9a1.7 1.7 0 0 0 2.5 1.1l.8-.5a1.7 1.7 0 0 1 2.4 2.4l-.5.8a1.7 1.7 0 0 0 1.1 2.5l.9.1a1.7 1.7 0 0 1 0 3.4l-.9.1a1.7 1.7 0 0 0-1.1 2.5l.5.8a1.7 1.7 0 0 1-2.4 2.4l-.8-.5a1.7 1.7 0 0 0-2.5 1.1l-.1.9a1.7 1.7 0 0 1-3.4 0l-.1-.9a1.7 1.7 0 0 0-2.5-1.1l-.8.5a1.7 1.7 0 0 1-2.4-2.4l.5-.8a1.7 1.7 0 0 0-1.1-2.5l-.9-.1a1.7 1.7 0 0 1 0-3.4l.9-.1a1.7 1.7 0 0 0 1.1-2.5l-.5-.8a1.7 1.7 0 0 1 2.4-2.4l.8.5a1.7 1.7 0 0 0 2.5-1.1l.1-.9z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  )
}

export function IconClose(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M6 6l12 12M18 6 6 18" />
    </svg>
  )
}

export function IconChevron(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="m8 10 4 4 4-4" />
    </svg>
  )
}

export function IconCopy(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="8" y="8" width="11" height="11" rx="1.5" />
      <path d="M5 14V6.5A1.5 1.5 0 0 1 6.5 5H14" />
    </svg>
  )
}

export function IconTrash(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M5 7h14M9 7V5h6v2M8 7l1 12h6l1-12" />
    </svg>
  )
}

export function IconPanel(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="4" y="5" width="16" height="14" rx="1.5" />
      <path d="M4 10h16" />
    </svg>
  )
}

export function IconPhoto(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="4" y="5" width="16" height="14" rx="1.5" />
      <circle cx="9" cy="10" r="1.5" />
      <path d="m8 16 3-3 2 2 3-4 2 5" />
    </svg>
  )
}

export function IconForward(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M14 8l5 4-5 4V8z" fill="currentColor" stroke="none" />
      <path d="M5 12h12" />
    </svg>
  )
}

export function IconEdit(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M4 20h4l10-10-4-4L4 16v4z" />
      <path d="m12 6 4 4" />
    </svg>
  )
}

export function IconFolder(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5v-10z" />
    </svg>
  )
}

export function IconTemplate(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="4" y="4" width="7" height="7" rx="1.5" />
      <rect x="13" y="4" width="7" height="4" rx="1.5" />
      <rect x="13" y="10" width="7" height="10" rx="1.5" />
      <rect x="4" y="13" width="7" height="7" rx="1.5" />
    </svg>
  )
}

export function IconTarget(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="8" />
      <circle cx="12" cy="12" r="4.5" />
      <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
    </svg>
  )
}

export function IconActivity(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M3 12h4l3-7 4 14 3-7h4" />
    </svg>
  )
}

export function IconRadio(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="1.8" />
      <path d="M8.5 15.5a5 5 0 0 1 0-7M15.5 8.5a5 5 0 0 1 0 7" />
      <path d="M5.6 18.4a9 9 0 0 1 0-12.8M18.4 5.6a9 9 0 0 1 0 12.8" />
    </svg>
  )
}

export function IconPlug(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M9 3v4M15 3v4" />
      <path d="M6.5 7h11v3.5a5.5 5.5 0 0 1-11 0V7z" />
      <path d="M12 16v5" />
    </svg>
  )
}

export function IconWrench(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M14.5 5.5a4 4 0 0 0 4.9 4.9l-8.9 8.9a2 2 0 0 1-2.8-2.8l8.9-8.9a4 4 0 0 1-2.1-2.1z" />
      <path d="M14.5 5.5 17 3l1.5 3.5L22 8l-2.6 2.4" />
    </svg>
  )
}

export function IconSend(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M21 3 3 10.5l7 2.5 2.5 7L21 3z" />
      <path d="m10 13 4.5-4.5" />
    </svg>
  )
}

export function IconUndo(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M9 14 4 9l5-5" />
      <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
    </svg>
  )
}

export function IconRedo(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="m15 14 5-5-5-5" />
      <path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13" />
    </svg>
  )
}

export function IconPlus(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  )
}

export function IconRefresh(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M20 11a8 8 0 0 0-14.6-4.5L4 8" />
      <path d="M4 4v4h4" />
      <path d="M4 13a8 8 0 0 0 14.6 4.5L20 16" />
      <path d="M20 20v-4h-4" />
    </svg>
  )
}

export function IconStop(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" />
    </svg>
  )
}

export function IconPause(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M9 6v12M15 6v12" />
    </svg>
  )
}

export function IconCheck(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="m5 12.5 4.5 4.5L19 7.5" />
    </svg>
  )
}

export function IconAlert(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M12 4 2.8 19.5h18.4L12 4z" />
      <path d="M12 10v4M12 17h.01" />
    </svg>
  )
}

export function IconInfo(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5M12 8h.01" />
    </svg>
  )
}

export function IconSidebar(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
      <path d="M9.5 4.5v15" />
    </svg>
  )
}

export function IconTerminal(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
      <path d="m7.5 10 2.5 2-2.5 2M12 15h4" />
    </svg>
  )
}

export function IconFlask(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M9.5 3.5h5M10.5 3.5v5L5 18.5A1.5 1.5 0 0 0 6.3 20.5h11.4a1.5 1.5 0 0 0 1.3-2L13.5 8.5v-5" />
      <path d="M7.5 14.5h9" />
    </svg>
  )
}

export function IconVariable(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M7 4C4.5 7 4.5 17 7 20M17 4c2.5 3 2.5 13 0 16" />
      <path d="m9.5 9 5 6M14.5 9l-5 6" />
    </svg>
  )
}

export function IconSun(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M18.7 5.3l-1.4 1.4M6.7 17.3l-1.4 1.4" />
    </svg>
  )
}

export function IconMoon(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" />
    </svg>
  )
}

export function IconMonitor(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="3" y="4.5" width="18" height="12" rx="1.5" />
      <path d="M9 20h6M12 16.5V20" />
    </svg>
  )
}

export function IconKey(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="8" cy="15" r="4" />
      <path d="m11 12 8.5-8.5M16 7l2.5 2.5M14 9l2 2" />
    </svg>
  )
}

export function IconUser(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="8.5" r="3.5" />
      <path d="M5 20a7 7 0 0 1 14 0" />
    </svg>
  )
}

export function IconBot(p: IconProps) {
  return (
    <svg {...base(p)}>
      <rect x="4.5" y="8" width="15" height="11" rx="2.5" />
      <path d="M12 4v4M9.5 13h.01M14.5 13h.01M2.5 13v2M21.5 13v2" />
    </svg>
  )
}

export function IconExternal(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M14 4h6v6M20 4l-9 9" />
      <path d="M18 14v4.5A1.5 1.5 0 0 1 16.5 20h-11A1.5 1.5 0 0 1 4 18.5v-11A1.5 1.5 0 0 1 5.5 6H10" />
    </svg>
  )
}

export function IconArrowRight(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M5 12h14M13 6l6 6-6 6" />
    </svg>
  )
}

export function IconPalette(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.2 0 1.8-.9 1.4-2l-.3-.8c-.4-1.1.4-2.2 1.6-2.2H17a3.5 3.5 0 0 0 3.5-3.5c0-4.7-3.8-8.5-8.5-8.5z" />
      <path d="M7.5 12h.01M9.5 8h.01M14.5 8h.01" />
    </svg>
  )
}

export function IconCode(p: IconProps) {
  return (
    <svg {...base(p)}>
      <path d="m8 7-5 5 5 5M16 7l5 5-5 5M13.5 5l-3 14" />
    </svg>
  )
}

export function IconDot(p: IconProps) {
  return (
    <svg {...base(p)}>
      <circle cx="12" cy="12" r="3" fill="currentColor" stroke="none" />
    </svg>
  )
}

export type NodeVisualCategory =
  | 'trigger'
  | 'logic'
  | 'data'
  | 'ai'
  | 'action'
  | 'integration'

export function visualCategoryForType(typeId: string): NodeVisualCategory {
  if (typeId.startsWith('trigger.')) return 'trigger'
  if (typeId.startsWith('logic.')) return 'logic'
  if (typeId.startsWith('data.')) return 'data'
  if (typeId.startsWith('ai.')) return 'ai'
  if (
    typeId.startsWith('telegram.') ||
    typeId.startsWith('telegram_user.')
  ) {
    return 'integration'
  }
  return 'action'
}

export function IconForType({
  typeId,
  size = 14,
}: {
  typeId: string
  size?: number
}) {
  switch (typeId) {
    case TYPE_IDS.TRIGGER_MANUAL:
      return <IconLightning size={size} />
    case TYPE_IDS.TRIGGER_SCHEDULE:
      return <IconClock size={size} />
    case TYPE_IDS.TRIGGER_WEBHOOK:
      return <IconWebhook size={size} />
    case TYPE_IDS.TRIGGER_TELEGRAM_USER_MESSAGE_RECEIVED:
      return <IconMessage size={size} />
    case TYPE_IDS.LOGIC_CONDITION:
      return <IconSplit size={size} />
    case TYPE_IDS.LOGIC_DELAY:
      return <IconClock size={size} />
    case TYPE_IDS.DATA_SET:
      return <IconBraces size={size} />
    case TYPE_IDS.DATA_TRANSFORM:
      return <IconArrows size={size} />
    case TYPE_IDS.HTTP_REQUEST:
      return <IconGlobe size={size} />
    case TYPE_IDS.AI_CHAT:
    case TYPE_IDS.AI_CLASSIFY:
    case TYPE_IDS.AI_ANALYZE:
    case TYPE_IDS.AI_IMAGE:
    case TYPE_IDS.AI_AUDIO:
    case TYPE_IDS.AI_VIDEO:
      return <IconSparkle size={size} />
    case TYPE_IDS.WEB_SEARCH:
      return <IconSearch size={size} />
    case TYPE_IDS.WEB_OPEN:
      return <IconExternal size={size} />
    case TYPE_IDS.WEB_EXTRACT:
      return <IconBraces size={size} />
    case TYPE_IDS.DEBUG_LOG:
      return <IconLog size={size} />
    case TYPE_IDS.TELEGRAM_SEND_MESSAGE:
    case TYPE_IDS.TELEGRAM_USER_SEND_MESSAGE:
      return <IconMessage size={size} />
    case TYPE_IDS.TELEGRAM_SEND_PHOTO:
      return <IconPhoto size={size} />
    case TYPE_IDS.TELEGRAM_SEND_DOCUMENT:
      return <IconPanel size={size} />
    case TYPE_IDS.TELEGRAM_USER_FORWARD_MESSAGE:
      return <IconForward size={size} />
    case TYPE_IDS.TELEGRAM_USER_EDIT_MESSAGE:
      return <IconEdit size={size} />
    case TYPE_IDS.TELEGRAM_USER_DELETE_MESSAGES:
      return <IconTrash size={size} />
    default:
      return <IconLog size={size} />
  }
}
