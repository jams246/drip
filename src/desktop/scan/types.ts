export interface ScanLocation {
  id: string
  name: string
  path: string
  kind: 'file' | 'folder'
}

export interface ScanSnapshot {
  jobId?: number
  id: string
  path: string
  kind: ScanLocation['kind']
  state: 'queued' | 'pending' | 'scanning' | 'completed' | 'empty' | 'error' | 'completed-with-errors'
  bytes: number
  chunks: number
  files: number
  skipped: number
  errors: number
  elapsedMs: number
  currentPath: string
  currentBytes: number
  currentSize: number
  error?: string
}

export type ScanRequest =
  | { type: 'select'; kind: ScanLocation['kind'] }
  | { type: 'remove'; id: string }
  | { type: 'pause'; paused: boolean }
  | { type: 'verify' }

export interface WatchHealth {
  id: string
  state: 'starting' | 'watching' | 'paused' | 'missing' | 'error'
  error: string
}

export type ScanEvent =
  | { type: 'hydrated'; locations: ScanLocation[]; scans: ScanSnapshot[] }
  | { type: 'selected'; item: ScanLocation }
  | { type: 'removed'; id: string }
  | { type: 'progress'; scan: ScanSnapshot }
  | { type: 'monitoring'; paused: boolean; verifying: boolean; health: WatchHealth[] }
  | { type: 'selection-ended' }
  | { type: 'error'; message: string }
