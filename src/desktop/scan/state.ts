import type { ScanLocation, ScanSnapshot } from './types'

export function initialScan(item: ScanLocation, state: 'pending' | 'queued' | 'error'): ScanSnapshot {
  return {
    id: item.id,
    path: item.path,
    kind: item.kind,
    state,
    bytes: 0,
    directories: 0,
    files: 0,
    skipped: 0,
    errors: 0,
    elapsedMs: 0,
    currentPath: '',
    currentBytes: 0,
    currentSize: 0,
    // Keep the error field in Perry's initial native object shape.
    error: ''
  }
}

export function isScanActive(scan: ScanSnapshot) {
  return scan.state === 'pending' || scan.state === 'scanning'
}

export function isScanFinished(scan: ScanSnapshot) {
  return scan.state !== 'queued' && !isScanActive(scan)
}
