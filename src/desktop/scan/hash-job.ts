import { lstatSync } from 'node:fs'
import { queryNames } from '#drip-window-icon'
import { recordDiagnostic } from '../diagnostics'
import { FileStore, normalizeFileId } from '../storage/files'
import { ChunkScanner } from './chunker'
import { inspectScanPath, inspectScopedScanPath } from './eligibility'
import { FileReader } from './reader'
import { type ScanJobResponse, type ScanJobStart, type ScanWorkerJob, initialJobResponse } from './worker-types'
// Log hash progress in 10% steps, capped at 100%.
const HASH_PROGRESS_STEP = 10
const HASH_PROGRESS_COMPLETE = 100

// oxlint-disable-next-line eslint/max-statements -- Hash job owns file state and diagnostic milestones through commit.
export function createHashJob(start: ScanJobStart, buffer: Uint8Array, sharedStorage?: FileStore): ScanWorkerJob {
  recordDiagnostic('hash.start', `job=${start.jobId} generation=${start.generation}`)
  const response = initialJobResponse(start)
  const storage = sharedStorage ?? new FileStore(start.databasePath, start.item.id)
  storage.reset(start.item.id)
  let reader: FileReader | undefined
  let scanner: ChunkScanner | undefined
  let ready = false
  let closed = false
  let skipped = false
  let sourcePath = start.target
  let lastProgress = 0

  function close() {
    if (closed) return
    closed = true
    try {
      scanner?.dispose()
      reader?.close()
    } finally {
      if (sharedStorage) storage.abortFile()
      else storage.close()
    }
  }

  function initialize() {
    const eligibility = inspectScopedScanPath(start.target, start.item.path, start.databasePath)
    if (eligibility === 'skip' && normalizeFileId(start.target) === normalizeFileId(start.item.path)) {
      throw new Error('Selected location is not currently supported; previous catalogue was retained.')
    }
    response.missing = eligibility === 'missing'
    skipped = eligibility === 'skip' || eligibility === 'excluded'
    if (response.missing || skipped) {
      response.skipped = skipped ? 1 : 0
      ready = true
    } else {
      const names: { longPath: string } = JSON.parse(queryNames(start.target))
      if (names.longPath) sourcePath = names.longPath
      reader = new FileReader(sourcePath)
      response.size = reader.size
      recordDiagnostic('hash.opened', `job=${start.jobId} size=${response.size}`)
      response.modifiedMs = reader.modifiedMs
      storage.beginFile(start.target)
      scanner = new ChunkScanner((offset: number, length: number, hash: string) => storage.stageChunk(offset, length, hash))
    }
  }

  try {
    initialize()
  } catch (error) {
    close()
    throw error
  }

  // oxlint-disable-next-line eslint/max-statements -- One read updates hash state and records bounded progress before returning.
  function step(): ScanJobResponse {
    if (closed) throw new Error('Hash job is closed.')
    if (ready) {
      response.type = 'ready'
      return response
    }
    const count = reader!.read(buffer)
    if (count > 0) {
      scanner!.update(buffer, count)
      response.bytes = scanner!.bytes
      response.chunks = scanner!.chunks
      const percent = Math.min(
        HASH_PROGRESS_COMPLETE,
        Math.floor(((response.bytes / response.size) * HASH_PROGRESS_COMPLETE) / HASH_PROGRESS_STEP) * HASH_PROGRESS_STEP
      )
      if (percent > lastProgress) {
        lastProgress = percent
        recordDiagnostic('hash.progress', `job=${start.jobId} percent=${percent} bytes=${response.bytes} chunks=${response.chunks}`)
      }
      return response
    }
    reader!.validate()
    scanner!.finish()
    response.bytes = scanner!.bytes
    response.chunks = scanner!.chunks
    response.files = 1
    recordDiagnostic('hash.complete', `job=${start.jobId} bytes=${response.bytes} chunks=${response.chunks}`)
    response.type = 'ready'
    ready = true
    return response
  }

  function commit(): ScanJobResponse {
    recordDiagnostic('hash.commit.start', `job=${start.jobId}`)
    if (!ready || closed) throw new Error('Hash job is not ready to commit.')
    const eligibility = inspectScopedScanPath(start.target, start.item.path, start.databasePath)
    if (skipped && eligibility === 'eligible') throw new Error('File became eligible before its skipped scan was committed.')
    if (response.missing) {
      if (eligibility !== 'missing') throw new Error('File appeared before deletion was committed.')
      if (normalizeFileId(start.target) !== normalizeFileId(start.item.path)) {
        validateSelection()
        storage.removeFile(start.target)
      }
    } else if (!skipped) {
      if (eligibility !== 'eligible') throw new Error('File is no longer eligible for hashing.')
      reader!.validate()
      storage.commitFile(sourcePath, response.bytes, response.modifiedMs)
    }
    response.type = 'done'
    close()
    recordDiagnostic('hash.commit.result', `job=${start.jobId} status=${response.type} skipped=${skipped} missing=${response.missing}`)
    return response
  }

  function validateSelection() {
    if (start.item.kind === 'folder' && (inspectScanPath(start.item.path, start.databasePath) !== 'eligible' || !lstatSync(start.item.path).isDirectory())) {
      throw new Error('Selected folder is unavailable; previous catalogue was retained.')
    }
  }

  return { step, commit, cancel: close }
}
