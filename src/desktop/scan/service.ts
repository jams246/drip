import { Worker } from 'node:worker_threads'
import { WatchStore } from '../storage/locations'
import { createStorageWrites } from '../storage/retry'
import { createScanQueue } from './queue'
import { isScanFinished } from './state'
import type { ScanEvent, ScanLocation, ScanSnapshot } from './types'

export function createScanService(databasePath: string, publish: (event: ScanEvent) => void) {
  const store = new WatchStore(databasePath)
  const writes = createStorageWrites()
  let closing = false
  let worker: Worker
  try {
    const saved = store.hydrate()
    worker = new Worker('../../../.perry/generated/scan-worker.ts')
    publish({ type: 'hydrated', locations: saved.locations, scans: saved.scans })
  } catch (error) {
    store.close()
    throw error
  }

  const queue = createScanQueue(
    (item: ScanLocation) => {
      // oxlint-disable-next-line unicorn/require-post-message-target-origin -- Node worker messages have no targetOrigin.
      worker.postMessage({ type: 'scan', item, databasePath })
    },
    publish,
    saveFailure
  )

  function saveFailure(scan: ScanSnapshot) {
    writes.enqueue(
      () => store.finishScan(scan),
      () => publish({ type: 'progress', scan }),
      (error) => {
        const message = `${scan.error || 'Scan failed.'} Could not save scan result: ${String(error)}`
        publish({ type: 'progress', scan: { ...scan, error: message } })
        publish({ type: 'error', message })
      }
    )
  }

  function fail(error: unknown) {
    if (!closing) queue.fail(error)
  }

  worker.on('message', (event: ScanEvent) => {
    if (closing) return
    if (event.type === 'error') {
      fail(event.message)
      return
    }
    if (event.type !== 'progress') return
    if (!isScanFinished(event.scan)) {
      queue.receive(event.scan)
      return
    }
    writes.enqueue(
      () => store.finishScan(event.scan),
      () => queue.receive(event.scan),
      fail
    )
  })
  worker.on('error', fail)
  worker.on('exit', (code: number) => fail(new Error(`Scan worker exited with code ${code}.`)))

  function select(item: ScanLocation) {
    writes.enqueue(
      () => {
        if (!queue.unavailable() && !queue.isScheduled(item.id)) store.saveLocation(item)
      },
      () => queue.enqueue(item),
      (error) => {
        publish({ type: 'error', message: `Could not save watch location: ${String(error)}` })
        publish({ type: 'selection-ended' })
      }
    )
  }

  function remove(id: string) {
    if (queue.isActive(id)) return
    writes.enqueue(
      () => {
        if (!queue.isActive(id)) store.removeLocation(id)
      },
      () => queue.remove(id),
      (error) => publish({ type: 'error', message: `Could not remove watch location: ${String(error)}` })
    )
  }

  function stop() {
    closing = true
    writes.stop()
    worker.unref()
    void worker.terminate()
    store.close()
  }

  return { select, remove, stop, unavailable: queue.unavailable }
}
