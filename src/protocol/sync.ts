export const MAX_REGION_BYTES = 1_048_576
export const REGION_PAGE_LIMIT = 1000

export interface RegionFingerprint {
  offset: number
  length: number
  hash: string
}

export interface Enrollment {
  token: string
  deviceId: string
  secret: string
  name: string
}

export interface Device {
  deviceId: string
  name: string
}

export interface RootRegistration {
  rootId: string
  name: string
  kind: 'folder'
}

export interface RootReceipt {
  rootId: string
  revision: number
}

export interface FileEntry {
  kind: 'file'
  path: string
  size: number
  modifiedMs: number
  hash: string
}

export interface DirectoryEntry {
  kind: 'directory'
  path: string
}

export type MirrorEntry = FileEntry | DirectoryEntry

export interface FileMove {
  from: string
  entry: FileEntry
}

export type Change = { kind: 'upsert'; entry: MirrorEntry } | { kind: 'delete'; path: string } | { kind: 'move'; moves: FileMove[] }

export interface Offer {
  operationId: string
  baseRevision: number
  change: Change
}

export interface OperationReceipt {
  status: 'offered' | 'publishing' | 'committed' | 'aborted'
  revision: number
  planComplete: boolean
  stageReady: boolean
  uploadRequired: boolean
}

export interface RegionPage {
  required: number[]
  next: number | null
}

export interface HeadsPage {
  revision: number
  heads: MirrorEntry[]
  next: string | null
}

export interface ApiError {
  code: string
  message: string
}
