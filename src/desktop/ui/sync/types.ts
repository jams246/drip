export interface WatchItem {
  id: string
  name: string
  path: string
  kind: 'folder' | 'file'
}

export interface SyncFile {
  id: string
  watchId: string
  name: string
  path: string
  kind: 'folder' | 'file'
  status: 'preparing' | 'syncing' | 'synced' | 'error'
  stage: string
  value?: number
  max: number
  detail: string
  errorActivityId?: string
}

export interface ProgressUpdate {
  fileId: string
  value?: number
  max?: number
  stage?: string
  status?: SyncFile['status']
}

export interface ActivityEntry {
  id: string
  time: string
  title: string
  detail: string
  path?: string
  severity: 'info' | 'success' | 'warning' | 'error'
  fileId?: string
}

export interface Connection {
  status: 'connected' | 'connecting' | 'disconnected' | 'error'
  url: string
  message?: string
  errorKind?: 'connection' | 'registration' | 'validation'
}
