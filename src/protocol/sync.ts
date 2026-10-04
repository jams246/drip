export const CHUNK_PROFILE = 'fastcdc-v1-blake3-256' as const

export interface Chunk {
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
  kind: 'file' | 'folder'
}

export interface RootReceipt {
  rootId: string
  revision: number
}

export interface FileHead {
  path: string
  size: number
  modifiedMs: number
  chunks: Chunk[]
}

export type Change = (FileHead & { kind: 'upsert'; profile: typeof CHUNK_PROFILE }) | { kind: 'delete'; path: string }

export interface Offer {
  operationId: string
  baseRevision: number
  change: Change
}

export interface OperationReceipt {
  status: 'offered' | 'publishing' | 'committed' | 'aborted'
  revision: number
  missing: string[]
}

export interface HeadsPage {
  revision: number
  heads: FileHead[]
  next: string | null
}

export interface ApiError {
  code: string
  message: string
}
