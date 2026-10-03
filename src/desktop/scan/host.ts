import { basename } from 'node:path'
import { Worker } from 'node:worker_threads'
import { WebView, onTerminate, openFileDialog, openFolderDialog, webviewEvaluateJs } from 'perry/ui'
import { createScanQueue } from './queue'
import { isScanFinished } from './state'
import type { ScanEvent, ScanLocation, ScanRequest } from './types'

const BRIDGE_INTERVAL_MS = 100
let nextBridgeSession = 0

function parseBridgeResponse(result: string): { ready: boolean; request?: ScanRequest } {
  const response: unknown = JSON.parse(result)
  if (!response || typeof response !== 'object' || !('ready' in response) || typeof response.ready !== 'boolean') {
    throw new Error('Invalid scan bridge response')
  }
  if (!response.ready) return { ready: false }
  if (!('request' in response) || response.request === null) return { ready: true }
  const request = response.request
  if (!request || typeof request !== 'object' || !('type' in request)) throw new Error('Invalid scan bridge request')
  if (request.type === 'remove' && 'id' in request && typeof request.id === 'string') {
    return { ready: true, request: { type: 'remove', id: request.id } }
  }
  if (request.type !== 'select' || !('kind' in request) || (request.kind !== 'file' && request.kind !== 'folder')) {
    throw new Error('Invalid scan selection kind')
  }
  return { ready: true, request: { type: 'select', kind: request.kind } }
}

export function startScanBridge(webview: ReturnType<typeof WebView>) {
  const worker = new Worker('../../../.perry/generated/scan-worker.ts')
  const events: ScanEvent[] = []
  const bridgeSession = ++nextBridgeSession
  let batchId = 1
  let retryBatch: ScanEvent[] | null = null
  let latestError: string | null = null
  let selecting = false
  let evaluating = false
  let closing = false

  function appendEvent(event: ScanEvent) {
    if (event.type === 'error') {
      latestError = event.message
      return
    }
    const previous = events[events.length - 1]
    if (event.type === 'progress' && previous?.type === 'progress' && previous.scan.id === event.scan.id && !isScanFinished(previous.scan)) {
      events[events.length - 1] = event
      return
    }
    events.push(event)
  }

  const queue = createScanQueue((item: ScanLocation) => {
    // oxlint-disable-next-line unicorn/require-post-message-target-origin -- Node worker messages have no targetOrigin.
    worker.postMessage({ type: 'scan', item })
  }, appendEvent)

  function finishWorkerError(error: unknown) {
    if (!closing) queue.fail(error)
  }
  worker.on('message', (event: ScanEvent) => {
    if (!closing && event.type === 'progress') queue.receive(event.scan)
  })
  worker.on('error', finishWorkerError)
  worker.on('exit', (code: number) => finishWorkerError(new Error(`Scan worker exited with code ${code}.`)))

  function select(kind: ScanLocation['kind']) {
    if (selecting) {
      appendEvent({ type: 'selection-ended' })
      return
    }
    const workerError = queue.unavailable()
    if (workerError) {
      appendEvent({ type: 'error', message: `Scan worker unavailable. Restart Drip. ${workerError}` })
      appendEvent({ type: 'selection-ended' })
      return
    }
    selecting = true
    function picked(path: string) {
      selecting = false
      if (closing) return
      if (typeof path !== 'string' || path.length === 0) {
        appendEvent({ type: 'selection-ended' })
        return
      }
      let id = path.split('\\').join('/').toLowerCase()
      while (id.endsWith('/')) id = id.slice(0, -1)
      queue.enqueue({ id, name: basename(path) || path, path, kind })
    }
    try {
      if (kind === 'file') openFileDialog(picked)
      else openFolderDialog(picked)
    } catch (error) {
      selecting = false
      appendEvent({ type: 'error', message: `Selection failed: ${String(error)}` })
      appendEvent({ type: 'selection-ended' })
    }
  }

  const bridgeTimer = setInterval(() => {
    if (evaluating || closing) return
    evaluating = true
    const batch = retryBatch ?? events.splice(0)
    if (!retryBatch && latestError) {
      batch.push({ type: 'error', message: latestError })
      latestError = null
    }
    // Retried receipts preserve already-consumed UI commands and terminal events.
    const script = `(() => { const bridge = window.__dripBridge; if (!bridge) return { ready: false }; const receipt = window.__dripScanReceipt; if (receipt?.session === ${bridgeSession} && receipt.id === ${batchId}) return receipt.response; const events = ${JSON.stringify(batch)}; for (const event of events) bridge.receive(event); const response = { ready: true, request: bridge.takeRequest() }; window.__dripScanReceipt = { session: ${bridgeSession}, id: ${batchId}, response }; return response; })()`
    webviewEvaluateJs(webview, script, (result: string) => {
      evaluating = false
      if (closing) return
      try {
        const response = parseBridgeResponse(result)
        if (!response.ready) {
          retryBatch = batch
          return
        }
        retryBatch = null
        batchId++
        if (response.request?.type === 'remove') queue.remove(response.request.id)
        if (response.request?.type === 'select') select(response.request.kind)
      } catch (error) {
        retryBatch = batch
        latestError = `Scan bridge failed: ${String(error)}`
      }
    })
  }, BRIDGE_INTERVAL_MS)
  onTerminate(() => {
    closing = true
    clearInterval(bridgeTimer)
    worker.unref()
    void worker.terminate()
  })
}
