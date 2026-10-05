import { IconAlert, IconCheck, IconClose, IconInfo } from '../ui/icons'
import { toneOf } from '../ui/primitives'

export function Toast({ message, onDismiss }: { message: string | null; onDismiss: () => void }) {
  if (!message) return null
  const tone = toneOf(message)
  return (
    <div className="toast-host" aria-live="polite">
      <div key={message} className={`toast toast--${tone}`} role={tone === 'error' ? 'alert' : 'status'}>
        <span className="toast__icon">
          {tone === 'error' ? <IconAlert size={15} /> : tone === 'success' ? <IconCheck size={15} /> : <IconInfo size={15} />}
        </span>
        <span className="toast__text">{message}</span>
        <button type="button" className="toast__close" onClick={onDismiss} aria-label="Dismiss">
          <IconClose size={13} />
        </button>
      </div>
    </div>
  )
}
