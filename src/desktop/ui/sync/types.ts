export interface ActivityEntry {
  id: string
  time: string
  title: string
  detail: string
  path?: string
  severity: 'info' | 'success' | 'warning' | 'error'
}

export interface Connection {
  status: 'connected' | 'connecting' | 'disconnected' | 'error'
  url: string
  message?: string
  errorKind?: 'connection' | 'registration' | 'validation'
}
