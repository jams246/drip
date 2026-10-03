import { parentPort as scanParentPort } from 'node:worker_threads'
import { scanLocation } from './traversal'
import type { ScanLocation, ScanSnapshot } from './types'

const SCAN_BUFFER_BYTES = 1_048_576
const scanBuffer = new Uint8Array(SCAN_BUFFER_BYTES)

scanParentPort!.on('message', (message: { type: string; item: ScanLocation }) => {
  if (message.type !== 'scan') return
  scanLocation(message.item, scanBuffer, (scan: ScanSnapshot) => {
    // oxlint-disable-next-line unicorn/require-post-message-target-origin -- Node worker messages have no targetOrigin.
    scanParentPort!.postMessage({ type: 'progress', scan })
  })
})
