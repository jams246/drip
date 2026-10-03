import { lstatSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import type { FileStore } from '../storage/files'
import { scanFile } from './file'
import type { ScanLocation, ScanSnapshot } from './types'

const PROGRESS_INTERVAL_MS = 100

export function scanLocation(item: ScanLocation, buffer: Uint8Array, publish: (scan: ScanSnapshot) => void, storage?: FileStore) {
  const started = Date.now()
  let lastPublished = 0
  let totalBytes = 0
  let totalChunks = 0
  let traversalFailed = false
  const scan: ScanSnapshot = {
    id: item.id,
    path: item.path,
    kind: item.kind,
    state: 'scanning',
    bytes: 0,
    chunks: 0,
    files: 0,
    skipped: 0,
    errors: 0,
    elapsedMs: 0,
    currentPath: item.path,
    currentBytes: 0,
    currentSize: 0,
    // Keep the error field in Perry's initial native object shape.
    error: ''
  }

  function update(force = false) {
    const now = Date.now()
    scan.elapsedMs = now - started
    if (!force && now - lastPublished < PROGRESS_INTERVAL_MS) return
    lastPublished = now
    publish(scan)
  }

  function fail(path: string, error: unknown) {
    scan.errors++
    scan.error = `${path}: ${String(error)}`
  }

  function scanRegularFile(path: string, size: number, modifiedMs: number) {
    scan.currentPath = path
    scan.currentSize = size
    scan.currentBytes = 0
    scan.bytes = totalBytes
    scan.chunks = totalChunks
    update()
    try {
      storage?.beginFile(path)
      const result = scanFile(
        path,
        buffer,
        (offset: number, length: number, hash: string) => storage?.stageChunk(offset, length, hash),
        (bytes: number, chunks: number) => {
          scan.currentBytes = bytes
          scan.bytes = totalBytes + bytes
          scan.chunks = totalChunks + chunks
          update()
        }
      )
      if (storage) {
        const current = lstatSync(path)
        if (!current.isFile() || result.bytes !== size || current.size !== size || current.mtimeMs !== modifiedMs) {
          throw new Error('File changed during scan.')
        }
        storage.commitFile(path, result.bytes, modifiedMs)
      }
    } catch (error) {
      storage?.abortFile()
      fail(path, error)
    } finally {
      totalBytes = scan.bytes
      totalChunks = scan.chunks
      scan.files++
    }
  }

  function visit(path: string) {
    try {
      const stats = lstatSync(path)
      if (stats.isSymbolicLink()) {
        scan.skipped++
        return
      }
      if (stats.isDirectory()) {
        for (const name of readdirSync(path)) visit(join(path, name))
        return
      }
      if (!stats.isFile()) {
        scan.skipped++
        return
      }
      scanRegularFile(path, stats.size, stats.mtimeMs)
    } catch (error) {
      traversalFailed = true
      fail(path, error)
    }
    update()
  }

  update(true)
  visit(item.path)
  if (item.kind === 'folder') {
    try {
      storage?.finishFolder(!traversalFailed)
    } catch (error) {
      fail(item.path, error)
    }
  }
  scan.currentPath = ''
  if (scan.errors > 0) scan.state = item.kind === 'folder' ? 'completed-with-errors' : 'error'
  else scan.state = scan.files === 0 ? 'empty' : 'completed'
  update(true)
}
