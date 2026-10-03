import { parentPort as scanParentPort } from 'node:worker_threads'
import { FileStore } from '../storage/files'
import { initialScan } from './state'
import { scanLocation } from './traversal'
import type { ScanLocation, ScanSnapshot } from './types'

const SCAN_BUFFER_BYTES = 1_048_576
const scanBuffer = new Uint8Array(SCAN_BUFFER_BYTES)

function publish(scan: ScanSnapshot) {
  // oxlint-disable-next-line unicorn/require-post-message-target-origin -- Node worker messages have no targetOrigin.
  scanParentPort!.postMessage({ type: 'progress', scan })
}

scanParentPort!.on('message', (message: { type: string; item: ScanLocation; databasePath: string }) => {
  if (message.type !== 'scan') return
  let storage: FileStore | undefined
  try {
    storage = new FileStore(message.databasePath, message.item.id)
    scanLocation(message.item, scanBuffer, publish, storage)
  } catch (error) {
    const scan = initialScan(message.item, 'error')
    scan.errors = 1
    scan.error = `Storage failed: ${String(error)}`
    publish(scan)
  } finally {
    storage?.close()
  }
})
