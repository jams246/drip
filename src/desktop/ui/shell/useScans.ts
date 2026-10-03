import { useCallback, useEffect, useRef, useState } from 'react'
import { initialScan, isScanActive, isScanFinished } from '../../scan/state'
import type { ScanEvent, ScanLocation, ScanRequest, ScanSnapshot } from '../../scan/types'
import type { ActivityEntry } from '../sync/types'

declare global {
  interface Window {
    __dripBridge?: {
      takeRequest(): ScanRequest | null
      receive(event: ScanEvent): void
    }
  }
}

interface ScanState {
  locations: ScanLocation[]
  scans: ScanSnapshot[]
  picking: boolean
  storage: 'loading' | 'ready' | 'error'
  error?: string
}

function selectLocation(previous: ScanState, item: ScanLocation): ScanState {
  const prior = previous.scans.find((scan) => scan.id === item.id)
  if (prior && !isScanFinished(prior)) return { ...previous, picking: false, error: undefined }
  return {
    ...previous,
    locations: [...previous.locations.filter((location) => location.id !== item.id), item],
    scans: [...previous.scans.filter((scan) => scan.id !== item.id), initialScan(item, 'queued')],
    picking: false,
    error: undefined
  }
}

function createScanActivity(scan: ScanSnapshot): Omit<ActivityEntry, 'id' | 'time'> {
  const failed = scan.state === 'error' || scan.state === 'completed-with-errors'
  let title = failed ? 'Scan completed with errors' : 'Scan completed'
  if (scan.state === 'error') title = 'Scan failed'
  return {
    title,
    detail: scan.error || `${scan.files} files, ${scan.chunks} chunks, ${scan.skipped} skipped.`,
    path: scan.path,
    severity: failed ? 'error' : 'success'
  }
}

export function useScans(onSelected: () => void, addActivity: (entry: Omit<ActivityEntry, 'id' | 'time'>) => void) {
  const [state, setState] = useState<ScanState>({ locations: [], scans: [], picking: false, storage: 'loading' })
  const current = useRef(state)
  const requests = useRef<ScanRequest[]>([])

  const publish = useCallback((next: ScanState) => {
    current.current = next
    setState(next)
  }, [])

  useEffect(() => {
    function updateProgress(scan: ScanSnapshot) {
      const previous = current.current
      const prior = previous.scans.find((entry) => entry.id === scan.id)
      if (!prior) return
      if (isScanFinished(prior) && !isScanFinished(scan)) return
      publish({ ...previous, scans: previous.scans.map((entry) => (entry.id === scan.id ? scan : entry)) })
      if (isScanFinished(scan) && scan.state !== prior.state) addActivity(createScanActivity(scan))
    }

    const bridge: NonNullable<Window['__dripBridge']> = {
      takeRequest() {
        return requests.current.shift() ?? null
      },
      receive(event) {
        const previous = current.current
        if (event.type === 'hydrated') {
          return publish({ locations: event.locations, scans: event.scans, picking: false, storage: 'ready' })
        }
        if (event.type === 'selection-ended') {
          publish({ ...previous, picking: false })
          return
        }
        if (event.type === 'error') {
          publish({
            ...previous,
            picking: false,
            storage: previous.storage === 'loading' ? 'error' : previous.storage,
            error: event.message
          })
          addActivity({ title: 'Scan service error', detail: event.message, severity: 'error' })
          return
        }
        if (event.type === 'selected') {
          publish(selectLocation(previous, event.item))
          onSelected()
          return
        }
        if (event.type === 'removed') {
          const item = previous.locations.find((location) => location.id === event.id)
          publish({
            ...previous,
            locations: previous.locations.filter((location) => location.id !== event.id),
            scans: previous.scans.filter((scan) => scan.id !== event.id)
          })
          if (item) addActivity({ title: 'Location removed', detail: 'The local file or folder is unchanged.', path: item.path, severity: 'info' })
          return
        }
        updateProgress(event.scan)
      }
    }
    window['__dripBridge'] = bridge
    return () => {
      if (window['__dripBridge'] === bridge) delete window['__dripBridge']
    }
  }, [onSelected, addActivity, publish])

  function select(kind: ScanLocation['kind']) {
    if (current.current.picking || current.current.storage !== 'ready') return
    requests.current.push({ type: 'select', kind })
    publish({ ...current.current, picking: true, error: undefined })
  }

  function remove(id: string) {
    const scan = current.current.scans.find((entry) => entry.id === id)
    if (!scan || isScanActive(scan)) return
    requests.current.push({ type: 'remove', id })
  }

  return { ...state, busy: state.storage === 'loading' || state.picking || state.scans.some(isScanActive), select, remove }
}
