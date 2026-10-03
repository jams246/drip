import { lstatSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { scanFile } from './file'
import type { ScanLocation, ScanSnapshot } from './types'

const PROGRESS_INTERVAL_MS = 100

export function scanLocation(item: ScanLocation, buffer: Uint8Array, publish: (scan: ScanSnapshot) => void) {
  const started = Date.now()
  let lastPublished = 0
  let totalBytes = 0
  let totalChunks = 0
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

  function scanRegularFile(path: string, size: number) {
    scan.currentPath = path
    scan.currentSize = size
    scan.currentBytes = 0
    scan.bytes = totalBytes
    scan.chunks = totalChunks
    update()
    try {
      const result = scanFile(
        path,
        buffer,
        () => {},
        (bytes: number, chunks: number) => {
          scan.currentBytes = bytes
          scan.bytes = totalBytes + bytes
          scan.chunks = totalChunks + chunks
          update()
        }
      )
      scan.currentBytes = result.bytes
      scan.bytes = totalBytes + result.bytes
      scan.chunks = totalChunks + result.chunks
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
      scanRegularFile(path, stats.size)
    } catch (error) {
      fail(path, error)
    }
    update()
  }

  update(true)
  visit(item.path)
  scan.currentPath = ''
  if (scan.errors > 0) scan.state = item.kind === 'folder' ? 'completed-with-errors' : 'error'
  else scan.state = scan.files === 0 ? 'empty' : 'completed'
  update(true)
}
