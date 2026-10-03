export interface ScanLocation {
  id: string
  name: string
  path: string
  kind: 'file' | 'folder'
}

export interface ScanSnapshot {
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

export type ScanRequest = { type: 'select'; kind: ScanLocation['kind'] } | { type: 'remove'; id: string }

export type ScanEvent =
  | { type: 'selected'; item: ScanLocation }
  | { type: 'removed'; id: string }
  | { type: 'progress'; scan: ScanSnapshot }
  | { type: 'selection-ended' }
  | { type: 'error'; message: string }
