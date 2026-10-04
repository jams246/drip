import { useRef, useState } from 'react'
import type { useScans } from '../shell/useScans'
import type { ActivityEntry } from '../sync/types'
import type { ConfirmationDetails } from './ConfirmationDialog'

type ConfirmationRequest = ConfirmationDetails & ({ action: 'verify' | 'clear' } | { action: 'remove'; id: string })

export function useConfirmation(scans: ReturnType<typeof useScans>, entries: readonly ActivityEntry[], clearActivity: () => void) {
  const [confirmation, setConfirmation] = useState<ConfirmationRequest | null>(null)
  const pending = useRef<ConfirmationRequest | null>(null)

  function isAvailable(request: ConfirmationRequest) {
    if (request.action === 'verify') return scans.storage !== 'loading' && !scans.paused && !scans.verifying && scans.locations.length > 0
    if (request.action === 'remove') {
      return scans.storage === 'ready' && scans.locations.some((item) => item.id === request.id) && scans.scans.some((scan) => scan.id === request.id)
    }
    return entries.length > 0
  }

  function ask(request: ConfirmationRequest) {
    if (pending.current || !isAvailable(request)) return
    pending.current = request
    setConfirmation(request)
  }

  function dismiss() {
    pending.current = null
    setConfirmation(null)
  }

  function confirm() {
    const request = pending.current
    dismiss()
    if (!request || !isAvailable(request)) return
    if (request.action === 'verify') return scans.verify()
    if (request.action === 'remove') return scans.remove(request.id)
    clearActivity()
  }

  function verify() {
    ask({
      action: 'verify',
      title: 'Verify all watched locations?',
      message: 'All watched files/folders will be reprocessed which can take a long time. Your local files will not be modified during this process.',
      confirmLabel: 'Verify all'
    })
  }

  function remove(id: string) {
    const item = scans.locations.find((location) => location.id === id)
    if (!item) return
    ask({
      action: 'remove',
      id,
      title: 'Remove this watch location?',
      message: `Removing "${item.path}" stops watching this location and cancels pending preparation. Local files and already synchronized server files are not deleted.`,
      confirmLabel: 'Remove'
    })
  }

  function clear() {
    ask({
      action: 'clear',
      title: 'Clear all activity?',
      message:
        'Displayed activity history will be cleared and cannot be recovered. Diagnostic and crash files will remain. New activity will continue to appear.',
      confirmLabel: 'Clear all activity'
    })
  }

  return { confirmation, available: confirmation !== null && isAvailable(confirmation), verify, remove, clear, confirm, dismiss }
}
