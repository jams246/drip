import { basename } from 'node:path'
import { releaseInstance, shellDrain, shellExit, shellHide, shellInit, shellOpen, shellPaused } from '#drip-window-icon'
import { WebView, onTerminate, openFileDialog, openFolderDialog, webviewEvaluateJs } from 'perry/ui'
import { normalizeFileId } from '../storage/files'
import { resolveDatabasePath } from '../storage/path'
import { createScanService } from './service'
import { isScanFinished } from './state'
import type { ScanEvent, ScanLocation, ScanRequest } from './types'

const BRIDGE_INTERVAL_MS = 100
let nextBridgeSession = 0
function closeDesktop() {
  releaseInstance()
  shellExit()
}

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
  if (request.type === 'verify') return { ready: true, request: { type: 'verify' } }
  if (request.type === 'pause' && 'paused' in request && typeof request.paused === 'boolean')
    return { ready: true, request: { type: 'pause', paused: request.paused } }
  if (request.type !== 'select' || !('kind' in request) || (request.kind !== 'file' && request.kind !== 'folder')) {
    throw new Error('Invalid scan selection kind')
  }
  return { ready: true, request: { type: 'select', kind: request.kind } }
}

export function startScanBridge(webview: ReturnType<typeof WebView>) {
  const events: ScanEvent[] = []
  const bridgeSession = ++nextBridgeSession
  let batchId = 1
  let retryBatch: ScanEvent[] | null = null
  let latestError: string | null = null
  let selecting = false
  let evaluating = false
  let closing = false
  let service: ReturnType<typeof createScanService> | undefined
  let initializationError = ''
  shellInit()

  function appendEvent(event: ScanEvent) {
    if (event.type === 'monitoring') shellPaused(event.paused)
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

  function exit() {
    if (closing) return
    closing = true
    clearInterval(bridgeTimer)
    if (service) service.stop(closeDesktop)
    else closeDesktop()
  }

  try {
    service = createScanService(resolveDatabasePath(), appendEvent)
  } catch (error) {
    initializationError = `Could not open Drip database: ${String(error)}`
    appendEvent({ type: 'error', message: initializationError })
  }

  function select(kind: ScanLocation['kind']) {
    if (selecting) {
      appendEvent({ type: 'selection-ended' })
      return
    }
    const workerError = initializationError || service?.unavailable()
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
      const id = normalizeFileId(path)
      service?.select({ id, name: basename(path) || path, path, kind })
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
    const commands: string[] = JSON.parse(shellDrain())
    for (const command of commands) {
      if (command === 'open' || command === 'tray-error') shellOpen()
      if (command === 'close' || command === 'minimize') shellHide()
      if (command === 'pause') service?.pause(!service.isPaused())
      if (command === 'exit' || command === 'shutdown') exit()
    }
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
        if (response.request?.type === 'remove') service?.remove(response.request.id)
        if (response.request?.type === 'select') select(response.request.kind)
        if (response.request?.type === 'pause') service?.pause(response.request.paused)
        if (response.request?.type === 'verify') service?.verifyAll()
      } catch (error) {
        retryBatch = batch
        latestError = `Scan bridge failed: ${String(error)}`
      }
    })
  }, BRIDGE_INTERVAL_MS)
  onTerminate(() => {
    closing = true
    clearInterval(bridgeTimer)
    service?.stop()
    releaseInstance()
  })
}
