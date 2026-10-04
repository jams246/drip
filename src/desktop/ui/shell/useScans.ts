import { useCallback, useEffect, useRef, useState } from 'react'
import { initialScan, isScanActive, isScanFinished } from '../../scan/state'
import type { ScanEvent, ScanLocation, ScanRequest, ScanSnapshot, WatchHealth } from '../../scan/types'
import type { ActivityEntry } from '../sync/types'
import type { SyncStatus } from '../../sync/types'

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
  paused: boolean
  verifying: boolean
  health: WatchHealth[]
  sync: SyncStatus
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
    detail: scan.error || `${scan.files} files, ${scan.directories} directories, ${scan.skipped} skipped.`,
    path: scan.path,
    severity: failed ? 'error' : 'success'
  }
}

export function useScans(addActivity: (entry: Omit<ActivityEntry, 'id' | 'time'>) => void) {
  const [state, setState] = useState<ScanState>({
    locations: [],
    scans: [],
    picking: false,
    storage: 'loading',
    paused: false,
    verifying: false,
    health: [],
    sync: { state: 'disconnected', url: '', pending: 0, transferredBytes: 0, message: 'Connect to a server to start synchronization.' }
  })
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
      if ((scan.jobId ?? 0) < (prior.jobId ?? 0)) return
      publish({ ...previous, scans: previous.scans.map((entry) => (entry.id === scan.id ? scan : entry)) })
      if (isScanFinished(scan) && scan.state !== prior.state) addActivity(createScanActivity(scan))
    }

    const bridge: NonNullable<Window['__dripBridge']> = {
      takeRequest() {
        return requests.current.shift() ?? null
      },
      // oxlint-disable-next-line eslint/max-statements -- Bridge routes the concrete scan, selection, and monitoring event variants.
      receive(event) {
        const previous = current.current
        if (event.type === 'sync') {
          publish({ ...previous, sync: event.status })
          if (event.status.state === 'error' && event.status.message !== previous.sync.message) {
            addActivity({ title: 'Synchronization failed', detail: event.status.message, severity: 'error' })
          }
          return
        }
        if (event.type === 'hydrated') {
          return publish({ ...previous, locations: event.locations, scans: event.scans, picking: false, storage: 'ready' })
        }
        if (event.type === 'monitoring') return publish({ ...previous, paused: event.paused, verifying: event.verifying, health: event.health })
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
  }, [addActivity, publish])

  function select(kind: ScanLocation['kind']) {
    if (current.current.picking || current.current.storage !== 'ready') return
    requests.current.push({ type: 'select', kind })
    publish({ ...current.current, picking: true, error: undefined })
  }

  function remove(id: string) {
    const latest = current.current
    if (latest.storage !== 'ready' || !latest.locations.some((location) => location.id === id) || !latest.scans.some((scan) => scan.id === id)) return
    if (requests.current.some((request) => request.type === 'remove' && request.id === id)) return
    requests.current.push({ type: 'remove', id })
  }

  function pause(paused: boolean) {
    requests.current.push({ type: 'pause', paused })
  }
  function verify() {
    const latest = current.current
    if (latest.storage === 'loading' || latest.paused || latest.verifying || latest.locations.length === 0) return
    if (requests.current.some((request) => request.type === 'verify')) return
    requests.current.push({ type: 'verify' })
    publish({ ...latest, verifying: true })
  }
  function connect(url: string, token: string) {
    requests.current.push({ type: 'connect', url, token })
    publish({ ...current.current, sync: { ...current.current.sync, state: 'connecting', message: 'Connecting to your server.' } })
  }
  return { ...state, busy: state.storage === 'loading' || state.picking || state.scans.some(isScanActive), select, remove, pause, verify, connect }
}
