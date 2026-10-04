import type { Change } from '../../protocol/sync'

export interface SyncCredentials {
  url: string
  deviceId: string
  secret: string
  name: string
  token: string
  confirmed: boolean
}

export interface SyncRoot {
  rootId: string
  name: string
  active: boolean
  registered: boolean
  revision: number
}

export interface FrozenOperation {
  rootId: string
  operationId: string
  baseRevision: number
  pathKey: string
  generation: number
  sourcePath: string
  change: Change
  abortRequested: boolean
}

export interface OperationMember {
  pathKey: string
  generation: number
  sourcePath: string
  change: Change
}

export interface SyncStatus {
  transferredBytes: number
  state: 'disconnected' | 'connecting' | 'idle' | 'uploading' | 'retrying' | 'error'
  url: string
  pending: number
  message: string
}
