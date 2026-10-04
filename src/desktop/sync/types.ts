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
  watchId: string
  rootId: string
  name: string
  kind: 'file' | 'folder'
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

export interface SyncStatus {
  state: 'disconnected' | 'connecting' | 'idle' | 'uploading' | 'retrying' | 'error'
  url: string
  pending: number
  message: string
}
