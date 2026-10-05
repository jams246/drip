import { type RefObject, useRef } from 'react'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from '../components/alert-dialog'
import { Alert, AlertDescription } from '../components/alert'

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
  const cancel = useRef<HTMLButtonElement>(null)
  const trigger = useRef<HTMLElement | null>(null)
  return (
    <AlertDialog
      open={confirmation !== null}
      onOpenChange={(open) => {
        if (!open) onDismiss()
      }}
    >
      {confirmation && (
        <AlertDialogContent
          className="max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] overflow-y-auto rounded-lg"
          onOpenAutoFocus={(event) => {
            event.preventDefault()
            trigger.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
            cancel.current?.focus()
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault()
            const element = trigger.current
            if (element?.isConnected && !element.matches(':disabled')) {
              element.focus()
              if (document.activeElement === element) return
            }
            fallbackFocus.current?.focus()
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>{confirmation.title}</AlertDialogTitle>
            <AlertDialogDescription className="break-words">{confirmation.message}</AlertDialogDescription>
          </AlertDialogHeader>
          {!available && (
            <Alert asChild>
              <output>
                <AlertDescription>This action is no longer available.</AlertDescription>
              </output>
            </Alert>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel ref={cancel}>Cancel</AlertDialogCancel>
            <AlertDialogAction disabled={!available} onClick={onConfirm}>
              {confirmation.confirmLabel}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      )}
    </AlertDialog>
  )
}
