import { initialScan, isScanFinished } from './state'
import type { ScanEvent, ScanLocation, ScanSnapshot } from './types'

export function createScanQueue(send: (item: ScanLocation) => void, publish: (event: ScanEvent) => void) {
  const waiting: ScanLocation[] = []
  let active: ScanLocation | null = null
  let lastScan: ScanSnapshot | null = null
  let workerError = ''

  function fail(error: unknown) {
    if (workerError) return
    workerError = String(error) || 'Scan worker failed.'
    if (active) {
      const scan = { ...(lastScan ?? initialScan(active, 'error')), state: 'error' as const, error: workerError }
      scan.errors++
      publish({ type: 'progress', scan })
    }
    for (const item of waiting) {
      const scan = initialScan(item, 'error')
      scan.errors = 1
      scan.error = workerError
      publish({ type: 'progress', scan })
    }
    waiting.length = 0
    active = null
    lastScan = null
    publish({ type: 'error', message: workerError })
  }

  function startNext() {
    if (active || workerError) return
    const item = waiting.shift()
    if (!item) return
    active = item
    lastScan = initialScan(item, 'pending')
    publish({ type: 'progress', scan: lastScan })
    try {
      send(item)
    } catch (error) {
      fail(error)
    }
  }

  function enqueue(item: ScanLocation) {
    if (workerError) {
      publish({ type: 'error', message: `Scan worker unavailable. Restart Drip. ${workerError}` })
      publish({ type: 'selection-ended' })
      return
    }
    publish({ type: 'selected', item })
    if (active?.id === item.id || waiting.some((entry) => entry.id === item.id)) return
    waiting.push(item)
    startNext()
  }

  function receive(scan: ScanSnapshot) {
    if (!active || scan.id !== active.id || workerError) return
    lastScan = scan
    publish({ type: 'progress', scan })
    if (!isScanFinished(scan)) return
    active = null
    lastScan = null
    startNext()
  }

  function remove(id: string) {
    if (active?.id === id) return
    const index = waiting.findIndex((item) => item.id === id)
    if (index >= 0) waiting.splice(index, 1)
    publish({ type: 'removed', id })
  }

  return { enqueue, receive, remove, fail, unavailable: () => workerError }
}
