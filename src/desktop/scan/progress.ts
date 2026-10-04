import { initialScan } from './state'
import type { ScanLocation, ScanSnapshot } from './types'

export function createScanProgress() {
  const snapshots = new Map<string, ScanSnapshot>()
  const started = new Map<string, number>()
  let baseBytes = 0

  function begin(item: ScanLocation, jobId: number, path: string) {
    let scan = snapshots.get(item.id)
    if (!scan || scan.state === 'completed' || scan.state === 'empty' || scan.state === 'error' || scan.state === 'completed-with-errors') {
      scan = initialScan(item, 'pending')
      started.set(item.id, Date.now())
    }
    const next = { ...scan, jobId, state: 'scanning' as const, currentPath: path, currentBytes: 0, currentSize: 0 }
    snapshots.set(item.id, next)
    baseBytes = next.bytes
    return next
  }

  function update(id: string, bytes: number, size: number) {
    const scan = snapshots.get(id)!
    const next = {
      ...scan,
      bytes: baseBytes + bytes,
      currentBytes: bytes,
      currentSize: size,
      elapsedMs: Date.now() - (started.get(id) ?? Date.now())
    }
    snapshots.set(id, next)
    return next
  }

  function finish(id: string, files: number, directories: number, skipped: number, errors: number, error: string, waiting: boolean) {
    const scan = snapshots.get(id)!
    let state: ScanSnapshot['state'] = waiting ? 'queued' : 'completed'
    const totalErrors = scan.errors + errors
    if (!waiting && totalErrors > 0) state = 'completed-with-errors'
    if (!waiting && totalErrors === 0 && scan.files + files + scan.directories + directories === 0) state = 'empty'
    const next = {
      ...scan,
      state,
      files: scan.files + files,
      directories: scan.directories + directories,
      skipped: scan.skipped + skipped,
      errors: totalErrors,
      currentPath: '',
      currentBytes: 0,
      currentSize: 0,
      error: error || scan.error || ''
    }
    snapshots.set(id, next)
    return next
  }

  return { begin, update, finish }
}
