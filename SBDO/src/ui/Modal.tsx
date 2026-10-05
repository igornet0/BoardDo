import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import { usePreferences } from '../settings/PreferencesContext'
import { IconAlert, IconClose } from './icons'

/** Stack of open modal ids — only the topmost one reacts to Escape. */
const stack: string[] = []

/** True while any modal is open; global canvas shortcuts check this. */
export function isModalOpen(): boolean {
  return stack.length > 0
}

export type ModalSize = 'sm' | 'md' | 'lg' | 'xl'

interface ModalProps {
  open: boolean
  onClose: () => void
  title: ReactNode
  subtitle?: ReactNode
  icon?: ReactNode
  size?: ModalSize
  /** Extra controls rendered in the header, left of the close button. */
  actions?: ReactNode
  footer?: ReactNode
  /** Body without padding — for layouts that manage their own columns. */
  flush?: boolean
  children: ReactNode
}

export function Modal({
  open,
  onClose,
  title,
  subtitle,
  icon,
  size = 'md',
  actions,
  footer,
  flush,
  children,
}: ModalProps) {
  const { t } = usePreferences()
  const id = useId()
  const dialogRef = useRef<HTMLDivElement>(null)
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose

  useEffect(() => {
    if (!open) return
    stack.push(id)
    const previouslyFocused = document.activeElement as HTMLElement | null
    dialogRef.current?.focus({ preventScroll: true })

    function onKey(ev: KeyboardEvent) {
      if (ev.key !== 'Escape' || stack[stack.length - 1] !== id) return
      ev.stopPropagation()
      ev.preventDefault()
      onCloseRef.current()
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      const idx = stack.lastIndexOf(id)
      if (idx >= 0) stack.splice(idx, 1)
      previouslyFocused?.focus?.({ preventScroll: true })
    }
  }, [open, id])

  if (!open) return null

  return createPortal(
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        ref={dialogRef}
        className={`modal modal--${size}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${id}-title`}
        tabIndex={-1}
      >
        <header className="modal__header">
          {icon && <span className="modal__icon">{icon}</span>}
          <div className="modal__titles">
            <h2 id={`${id}-title`}>{title}</h2>
            {subtitle && <p>{subtitle}</p>}
          </div>
          <div className="modal__actions">
            {actions}
            <button
              type="button"
              className="icon-btn"
              onClick={onClose}
              aria-label={t('common.close')}
              title={`${t('common.close')} · Esc`}
            >
              <IconClose size={16} />
            </button>
          </div>
        </header>
        <div className={`modal__body${flush ? ' modal__body--flush' : ''}`}>
          {children}
        </div>
        {footer && <footer className="modal__footer">{footer}</footer>}
      </div>
    </div>,
    document.body,
  )
}

// ---------------------------------------------------------------------------
// Confirm dialog — promise-based replacement for window.confirm

interface ConfirmOptions {
  title: string
  message?: string
  confirmLabel?: string
  danger?: boolean
}

type ConfirmFn = (opts: ConfirmOptions) => Promise<boolean>

const ConfirmContext = createContext<ConfirmFn | null>(null)

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const { t } = usePreferences()
  const [pending, setPending] = useState<
    (ConfirmOptions & { resolve: (ok: boolean) => void }) | null
  >(null)

  const confirm = useCallback<ConfirmFn>(
    (opts) => new Promise((resolve) => setPending({ ...opts, resolve })),
    [],
  )

  function settle(ok: boolean) {
    pending?.resolve(ok)
    setPending(null)
  }

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      <Modal
        open={pending != null}
        onClose={() => settle(false)}
        size="sm"
        icon={
          <span className={pending?.danger ? 'tone-error' : 'tone-warning'}>
            <IconAlert size={18} />
          </span>
        }
        title={pending?.title ?? ''}
        footer={
          <>
            <button type="button" className="btn" onClick={() => settle(false)}>
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className={pending?.danger ? 'btn btn--danger-solid' : 'btn btn--primary'}
              autoFocus
              onClick={() => settle(true)}
            >
              {pending?.confirmLabel ?? t('common.confirm')}
            </button>
          </>
        }
      >
        {pending?.message && <p className="confirm__text">{pending.message}</p>}
      </Modal>
    </ConfirmContext.Provider>
  )
}

export function useConfirm(): ConfirmFn {
  const fn = useContext(ConfirmContext)
  if (!fn) throw new Error('useConfirm must be used inside ConfirmProvider')
  return fn
}
