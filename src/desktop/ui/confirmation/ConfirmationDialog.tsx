import { type RefObject, useEffect, useRef } from 'react'
import './confirmation.css'

export interface ConfirmationDetails {
  title: string
  message: string
  confirmLabel: string
}

interface ConfirmationDialogProps {
  confirmation: ConfirmationDetails | null
  available: boolean
  fallbackFocus: RefObject<HTMLElement | null>
  onConfirm: () => void
  onDismiss: () => void
}

export function ConfirmationDialog({ confirmation, available, fallbackFocus, onConfirm, onDismiss }: ConfirmationDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null)
  const cancel = useRef<HTMLButtonElement>(null)
  const open = confirmation !== null

  useEffect(() => {
    const element = dialog.current
    if (!open || !element) return undefined
    const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const fallback = fallbackFocus.current
    element.showModal()
    cancel.current?.focus()
    return () => {
      element.close()
      if (trigger?.isConnected && !trigger.matches(':disabled')) {
        trigger.focus()
        if (document.activeElement === trigger) return
      }
      fallback?.focus()
    }
  }, [open, fallbackFocus])

  function close(action: () => void) {
    if (!dialog.current?.open) return
    dialog.current.close()
    action()
  }

  return (
    <dialog
      className="confirmation"
      ref={dialog}
      aria-labelledby="confirmation-title"
      aria-describedby="confirmation-message"
      onCancel={(event) => {
        event.preventDefault()
        close(onDismiss)
      }}
    >
      <h2 className="confirmation__title" id="confirmation-title">
        {confirmation?.title}
      </h2>
      <p className="confirmation__message" id="confirmation-message">
        {confirmation?.message}
      </p>
      {open && !available && <output className="confirmation__notice">This action is no longer available.</output>}
      <div className="confirmation__actions">
        <button className="button button--secondary" type="button" ref={cancel} onClick={() => close(onDismiss)}>
          Cancel
        </button>
        <button className="button button--primary" type="button" disabled={!available} onClick={() => close(onConfirm)}>
          {confirmation?.confirmLabel}
        </button>
      </div>
    </dialog>
  )
}
