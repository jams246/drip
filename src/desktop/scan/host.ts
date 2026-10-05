import { basename } from 'node:path'
import { releaseInstance, shellDrain, shellExit, shellHide, shellInit, shellOpen, shellPaused } from '#drip-window-icon'
import { WebView, onTerminate, openFileDialog, openFolderDialog, webviewEvaluateJs } from 'perry/ui'
import { normalizeFileId } from '../storage/files'
import { resolveDatabasePath } from '../storage/path'
import { createScanService } from './service'
import { createSyncDriver } from '../sync/driver'
import { recordDiagnostic } from '../diagnostics'
import { isScanFinished } from './state'
import type { ScanEvent, ScanLocation, ScanRequest } from './types'

const BRIDGE_INTERVAL_MS = 100
let nextBridgeSession = 0
function closeDesktop() {
  recordDiagnostic('desktop.exit')
  releaseInstance()
  shellExit()
  process.exit(0)
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
  if (request.type === 'connect' && 'url' in request && typeof request.url === 'string' && 'token' in request && typeof request.token === 'string') {
    return { ready: true, request: { type: 'connect', url: request.url, token: request.token } }
  }
  if (request.type === 'pause' && 'paused' in request && typeof request.paused === 'boolean')
    return { ready: true, request: { type: 'pause', paused: request.paused } }
  if (request.type !== 'select' || !('kind' in request) || (request.kind !== 'file' && request.kind !== 'folder')) {
    throw new Error('Invalid scan selection kind')
  }
  return { ready: true, request: { type: 'select', kind: request.kind } }
}

// oxlint-disable-next-line eslint/max-statements -- One desktop bridge owns scan, sync, and shell lifecycles.
export function startScanBridge(webview: ReturnType<typeof WebView>) {
  recordDiagnostic('desktop.bridge.start')
  const events: ScanEvent[] = []
  const bridgeSession = ++nextBridgeSession
  let batchId = 1
  let retryBatch: ScanEvent[] | null = null
  let latestError: string | null = null
  let selecting = false
  let evaluating = false
  let closing = false
  let service: ReturnType<typeof createScanService> | undefined
  let sync: ReturnType<typeof createSyncDriver> | undefined
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
    recordDiagnostic('desktop.shutdown')
    closing = true
    clearInterval(bridgeTimer)
    void (async () => {
      await sync?.stop()
      if (service) service.stop(closeDesktop)
      else closeDesktop()
    })()
  }

  try {
    const databasePath = resolveDatabasePath()
    service = createScanService(databasePath, appendEvent)
    sync = createSyncDriver({
      databasePath,
      publish: (status) => appendEvent({ type: 'sync', status }),
      rescan: (id, path) => service?.rescan(id, path),
      busy: (id) => Boolean(service?.busy(id))
    })
  } catch (error) {
    recordDiagnostic('desktop.initialize.error', error instanceof Error ? error.name : 'unknown')
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

  const pumpBridge = () => {
    const commands: string[] = JSON.parse(shellDrain())
    for (const command of commands) {
      if (command === 'open' || command === 'tray-error') shellOpen()
      if (command === 'close' || command === 'minimize') shellHide()
      if (command === 'pause') service?.pause(!service.isPaused())
      if (command === 'exit' || command === 'shutdown') exit()
    }
    if (!closing) void sync?.tick()
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
        if (response.request?.type === 'connect') void sync?.connect(response.request.url, response.request.token)
      } catch (error) {
        const name = error instanceof Error ? error.name : 'unknown'
        recordDiagnostic('desktop.bridge.error', `error=${name} batch=${batchId} events=${batch.length} resultLength=${result.length}`)
        retryBatch = batch
        latestError = `Scan bridge failed: ${String(error)}`
      }
    })
  }
  const bridgeTimer = setInterval(pumpBridge, BRIDGE_INTERVAL_MS)
  onTerminate(() => {
    recordDiagnostic('desktop.terminate')
    closing = true
    clearInterval(bridgeTimer)
    void sync?.stop()
    service?.stop()
    // Forced termination cannot await callbacks. Frozen operations are durable, and Windows keeps the mutex until process death.
  })
  pumpBridge()
}
