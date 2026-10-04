import { useState } from 'react'
import type { Connection } from '../sync/types'
import type { SyncStatus } from '../../sync/types'

export function useConnection(sync: SyncStatus, sendConnection: (url: string, token: string) => void) {
  const [serverUrl, setServerUrl] = useState<string | undefined>(undefined)
  const [registrationCode, setRegistrationCode] = useState('')
  let status: Connection['status'] = 'connected'
  if (sync.state === 'disconnected') status = 'disconnected'
  if (sync.state === 'connecting') status = 'connecting'
  if (sync.state === 'error' || sync.state === 'retrying') status = 'error'
  const connection: Connection = { status, url: sync.url, message: sync.message }
  function connect(url: string, token: string) {
    sendConnection(url, token)
    setRegistrationCode('')
  }
  return { connection, serverUrl: serverUrl ?? sync.url, registrationCode, setServerUrl, setRegistrationCode, connect }
}
