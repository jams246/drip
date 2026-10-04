import { lstatSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { queryNames } from '#drip-window-icon'
import { FileStore, normalizeFileId } from '../storage/files'
import { type PathQuery, inspectScanPath, inspectScopedScanPath } from './eligibility'
import { type ScanJobResponse, type ScanJobStart, type ScanWorkerJob, initialJobResponse } from './worker-types'

const INVENTORY_ENTRY_LIMIT = 16

// oxlint-disable-next-line eslint/max-statements -- One inventory lifecycle owns traversal, pruning safety, commit, and cancellation state.
export function createInventoryJob(start: ScanJobStart, query?: PathQuery, sharedStorage?: FileStore): ScanWorkerJob {
  const storage = sharedStorage ?? new FileStore(start.databasePath, start.item.id)
  storage.reset(start.item.id)
  const response = initialJobResponse(start)
  const directories: string[] = []
  const batch = { directory: '', paths: [start.target], index: 0 }
  let phase = 'walk'
  let pruningSafe = true
  let lastUnseen = ''
  const selectedRoot = normalizeFileId(start.target) === normalizeFileId(start.item.path)

  function close() {
    if (phase === 'closed') return
    phase = 'closed'
    if (sharedStorage) storage.abortFile()
    else storage.close()
  }

  function skip(path: string) {
    if (selectedRoot && normalizeFileId(path) === normalizeFileId(start.target)) {
      throw new Error('Selected location is not currently supported; previous catalogue was retained.')
    }
    response.skipped++
    pruningSafe = false
  }

  function inspectEligible(path: string) {
    const stats = lstatSync(path)
    if (stats.isSymbolicLink()) {
      skip(path)
      return
    }
    let sourcePath = path
    if (path === start.target) {
      const names: { longPath: string } = JSON.parse(queryNames(path))
      if (names.longPath) sourcePath = names.longPath
    }
    if (stats.isDirectory()) {
      if (start.item.kind === 'file') throw new Error('Selected file is now a folder.')
      storage.observeDirectory(sourcePath)
      directories.push(sourcePath)
      response.directories++
      return
    }
    if (!stats.isFile()) {
      skip(path)
      return
    }
    recordFile(path, sourcePath, stats.size, stats.mtimeMs)
  }

  function recordFile(path: string, sourcePath: string, size: number, modifiedMs: number) {
    if (path === start.target && selectedRoot && start.item.kind === 'folder') throw new Error('Selected folder is now a file.')
    storage.markSeen(sourcePath)
    const needsHash = start.force || storage.needsHash(sourcePath, size, modifiedMs)
    if (!needsHash) storage.observeFile(sourcePath)
    response.entries.push({
      path: sourcePath,
      size,
      modifiedMs,
      needsHash
    })
    response.files++
  }

  function inspect(path: string) {
    const eligibility =
      path === start.target ? inspectScopedScanPath(path, start.item.path, start.databasePath, query) : inspectScanPath(path, start.databasePath, query)
    if (eligibility === 'eligible') {
      inspectEligible(path)
      return
    }
    if (eligibility === 'missing') {
      if (path === start.target) {
        response.missing = true
        if (selectedRoot) pruningSafe = false
      }
      return
    }
    if (eligibility === 'excluded') {
      response.skipped++
      if (path === start.target) pruningSafe = false
      return
    }
    skip(path)
  }

  function fail(path: string, error: unknown) {
    pruningSafe = false
    response.errors++
    response.error = `${path}: ${String(error)}`
  }

  function inspectBatch() {
    if (batch.index >= batch.paths.length) return
    try {
      if (batch.directory && inspectScopedScanPath(batch.directory, start.item.path, start.databasePath, query) !== 'eligible') {
        skip(batch.directory)
        batch.index = batch.paths.length
        return
      }
    } catch (error) {
      fail(batch.directory, error)
      batch.index = batch.paths.length
      return
    }
    let checked = 0
    while (batch.index < batch.paths.length && checked < INVENTORY_ENTRY_LIMIT) {
      const path = batch.paths[batch.index++]
      checked++
      try {
        inspect(path)
      } catch (error) {
        fail(path, error)
      }
    }
  }

  function enumerateNext() {
    const directory = directories.pop()!
    try {
      if (inspectScopedScanPath(directory, start.item.path, start.databasePath, query) !== 'eligible') {
        skip(directory)
        return
      }
      // Perry enumerates this directory in one synchronous call; inspect its names in later steps.
      batch.paths = readdirSync(directory).map((name: string) => join(directory, name))
      batch.directory = directory
      batch.index = 0
    } catch (error) {
      fail(directory, error)
    }
  }

  function step(): ScanJobResponse {
    if (phase === 'closed') throw new Error('Inventory job is closed.')
    response.entries = []
    if (phase === 'ready') return response
    response.type = 'entries'
    if (phase === 'unseen') {
      inspectUnseen()
      return response
    }
    if (batch.index >= batch.paths.length && directories.length > 0) {
      enumerateNext()
      return response
    }
    inspectBatch()
    if (batch.index >= batch.paths.length && directories.length === 0) {
      phase = pruningSafe && response.errors === 0 && start.item.kind === 'folder' ? 'unseen' : 'ready'
      if (phase === 'ready') {
        response.type = 'ready'
      }
    }
    return response
  }

  function inspectUnseen() {
    const unseen = storage.unseenPaths(start.target, lastUnseen, INVENTORY_ENTRY_LIMIT)
    response.entries = unseen.map((path) => ({ path, size: 0, modifiedMs: 0, needsHash: true }))
    if (unseen.length > 0) lastUnseen = unseen[unseen.length - 1]
    else {
      phase = 'ready'
      response.type = 'ready'
    }
  }

  function commit(): ScanJobResponse {
    if (phase !== 'ready') throw new Error('Inventory job is not ready to commit.')
    if (pruningSafe) {
      const eligibility = inspectScopedScanPath(start.target, start.item.path, start.databasePath, query)
      if (response.missing && eligibility !== 'missing') throw new Error('Inventory scope appeared before deletion was committed.')
      if (!response.missing && eligibility !== 'eligible') throw new Error('Inventory scope changed before commit.')
    }
    if (selectedRoot) storage.coverage(pruningSafe && response.errors === 0 && response.skipped === 0 && !response.missing)
    response.entries = []
    response.type = 'done'
    close()
    return response
  }

  return { step, commit, cancel: close }
}
