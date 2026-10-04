export interface Chunk {
  offset: number
  length: number
  hash: string
}

export interface RootRegistration {
  rootId: string
  name: string
  kind: 'file' | 'folder'
}

export interface Root extends RootRegistration {
  deviceId: string
  revision: number
  retired: boolean
}

export interface FileHead {
  path: string
  size: number
  modifiedMs: number
  chunks: Chunk[]
}

export type Change = (FileHead & { kind: 'upsert'; profile: 'fastcdc-v1-blake3-256' }) | { kind: 'delete'; path: string }
export interface Offer {
  operationId: string
  baseRevision: number
  change: Change
}

export interface Operation extends Offer {
  rootId: string
  deviceId: string
  status: 'offered' | 'publishing' | 'committed' | 'aborted'
  revision: number
}

export interface OperationReceipt {
  status: Operation['status']
  revision: number
  missing: string[]
}

export interface HeadsPage {
  revision: number
  heads: FileHead[]
  next: string | null
}

export const canonicalPath = (path: string) => path.toLowerCase()
