import { initialScan, isScanFinished } from './state'
import type { ScanEvent, ScanLocation, ScanSnapshot } from './types'

export function createBridgeState() {
  const locations = new Map<string, ScanLocation>()
  const scans = new Map<string, ScanSnapshot>()
  let hydrated = false
  let monitoring: ScanEvent | undefined
  let sync: ScanEvent | undefined
  let error: ScanEvent | undefined

  function record(event: ScanEvent) {
    if (event.type === 'hydrated') {
      hydrated = true
      locations.clear()
      scans.clear()
      for (const item of event.locations) locations.set(item.id, item)
      for (const scan of event.scans) scans.set(scan.id, scan)
    }
    if (event.type === 'selected') {
      locations.set(event.item.id, event.item)
      const prior = scans.get(event.item.id)
      if (!prior || isScanFinished(prior)) scans.set(event.item.id, initialScan(event.item, 'queued'))
      error = undefined
    }
    if (event.type === 'removed') {
      locations.delete(event.id)
      scans.delete(event.id)
    }
    if (event.type === 'progress' && locations.has(event.scan.id)) {
      const prior = scans.get(event.scan.id)
      if ((event.scan.jobId ?? 0) >= (prior?.jobId ?? 0)) scans.set(event.scan.id, event.scan)
    }
    if (event.type === 'monitoring') monitoring = event
    if (event.type === 'sync') sync = event
    if (event.type === 'error') error = event
  }

  function snapshot(): ScanEvent[] {
    const events: ScanEvent[] = []
    if (hydrated) events.push({ type: 'hydrated', locations: [...locations.values()], scans: [...scans.values()] })
    if (monitoring) events.push(monitoring)
    if (sync) events.push(sync)
    if (error) events.push(error)
    return events
  }
  return { record, snapshot }
}
