import { parentPort as scanParentPort } from 'node:worker_threads'
import { recordDiagnostic } from '../diagnostics'
import { FileStore } from '../storage/files'
import { createHashJob } from './hash-job'
import { createInventoryJob } from './traversal'
import { type ScanJobCommand, type ScanJobResponse, type ScanJobStart, type ScanWorkerJob, initialJobResponse } from './worker-types'

const SCAN_BUFFER_BYTES = 1_048_576
const scanBuffer = new Uint8Array(SCAN_BUFFER_BYTES)
let activeJob: ScanWorkerJob | undefined
let activeResponse: ScanJobResponse | undefined
let workerStorage: FileStore | undefined
let workerDatabasePath = ''
recordDiagnostic('scan.worker.ready')

function publish(response: ScanJobResponse) {
  // oxlint-disable-next-line unicorn/require-post-message-target-origin -- Node worker messages have no targetOrigin.
  scanParentPort!.postMessage(response)
}

function startJob(message: ScanJobStart) {
  activeResponse = initialJobResponse(message)
  activeJob?.cancel()
  activeJob = undefined
  if (workerStorage && workerDatabasePath !== message.databasePath) closeStorage()
  if (!workerStorage) {
    workerStorage = new FileStore(message.databasePath, message.item.id)
    workerDatabasePath = message.databasePath
  }
  activeJob = message.kind === 'inventory' ? createInventoryJob(message, undefined, workerStorage) : createHashJob(message, scanBuffer, workerStorage)
  publish(activeResponse)
}

function closeStorage() {
  const storage = workerStorage
  workerStorage = undefined
  workerDatabasePath = ''
  storage?.close()
}

function cancelJob(jobId: number) {
  activeResponse ??= initialJobResponse({ jobId, generation: 0, target: '' })
  activeResponse.jobId = jobId
  activeJob?.cancel()
  activeJob = undefined
  closeStorage()
  activeResponse.type = 'cancelled'
  publish(activeResponse)
  activeResponse = undefined
}

function failJob(error: unknown) {
  recordDiagnostic('scan.worker.job.error', `job=${activeResponse?.jobId ?? 0} error=${error instanceof Error ? error.name : 'unknown'}`)
  let failure = String(error)
  try {
    activeJob?.cancel()
  } catch (cleanupError) {
    failure += ` Cleanup: ${String(cleanupError)}`
  }
  activeJob = undefined
  try {
    closeStorage()
  } catch (cleanupError) {
    failure += ` Storage cleanup: ${String(cleanupError)}`
  }
  if (activeResponse) {
    activeResponse.type = 'error'
    activeResponse.errors++
    activeResponse.error = failure
    publish(activeResponse)
    activeResponse = undefined
  }
}

function handleCommand(message: ScanJobCommand) {
  if (message.type === 'start') {
    startJob(message)
    return
  }
  if (message.type === 'cancel') {
    cancelJob(message.jobId)
    return
  }
  if (!activeJob) return
  activeResponse = message.type === 'step' ? activeJob.step() : activeJob.commit()
  publish(activeResponse)
  if (message.type === 'commit') {
    activeJob = undefined
    activeResponse = undefined
  }
}

scanParentPort!.on('message', (message: ScanJobCommand) => {
  if (message.type !== 'start' && message.jobId !== activeResponse?.jobId && !(message.type === 'cancel' && !activeJob)) return
  try {
    handleCommand(message)
  } catch (error) {
    failJob(error)
  }
})
