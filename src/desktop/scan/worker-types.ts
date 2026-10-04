import type { ScanLocation } from './types'

export interface ScanJobStart {
  type: 'start'
  jobId: number
  item: ScanLocation
  databasePath: string
  target: string
  kind: 'inventory' | 'hash'
  force: boolean
  generation: number
}

export type ScanJobCommand = ScanJobStart | { type: 'step' | 'commit' | 'cancel'; jobId: number }

export interface InventoryEntry {
  path: string
  size: number
  modifiedMs: number
  needsHash: boolean
}

export interface ScanJobResponse {
  type: 'entries' | 'ready' | 'progress' | 'done' | 'cancelled' | 'error'
  jobId: number
  generation: number
  path: string
  entries: InventoryEntry[]
  missing: boolean
  bytes: number
  chunks: number
  files: number
  skipped: number
  errors: number
  error: string
  size: number
  modifiedMs: number
}

export interface ScanWorkerJob {
  step(): ScanJobResponse
  commit(): ScanJobResponse
  cancel(): void
}

export function initialJobResponse(start: Pick<ScanJobStart, 'jobId' | 'generation' | 'target'>): ScanJobResponse {
  return {
    type: 'progress',
    jobId: start.jobId,
    generation: start.generation,
    path: start.target,
    entries: [],
    missing: false,
    bytes: 0,
    chunks: 0,
    files: 0,
    skipped: 0,
    errors: 0,
    error: '',
    size: 0,
    modifiedMs: 0
  }
}
