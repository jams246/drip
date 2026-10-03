import { useRef, useState } from 'react'
import type { ActivityEntry, Connection } from '../sync/types'

const connectionDelay = 700
const sampleUrl = 'https://sync.drip.example'

function connectionResult(url: string, code: string): Connection {
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:')
      return { status: 'error', url, errorKind: 'validation', message: 'Enter a server URL starting with https:// or http://.' }
    if (parsed.hostname === 'offline.drip.example')
      return { status: 'error', url, errorKind: 'connection', message: 'Could not reach the server. Check the server URL and try again.' }
    if (code !== 'DRIP-DEMO')
      return { status: 'error', url, errorKind: 'registration', message: 'Registration failed. Check your registration code and try again.' }
    return { status: 'connected', url, message: 'Connected. Your computer is registered with this server.' }
  } catch {
    return { status: 'error', url, errorKind: 'validation', message: 'Enter a valid server URL.' }
  }
}

export function useConnection(paused: boolean, onPauseChange: (paused: boolean) => void, addActivity: (entry: Omit<ActivityEntry, 'id' | 'time'>) => void) {
  const [connection, setConnection] = useState<Connection>({ status: 'connected', url: sampleUrl })
  const [serverUrl, setServerUrl] = useState(sampleUrl)
  const [registrationCode, setRegistrationCode] = useState('')
  const connecting = useRef(false)
  const pauseBeforeConnection = useRef(paused)

  async function connect(url: string, code: string) {
    if (connecting.current) return
    connecting.current = true
    if (connection.status === 'connected') pauseBeforeConnection.current = paused
    onPauseChange(true)
    setConnection({ status: 'connecting', url, message: 'Connecting to your server…' })
    addActivity({ title: 'Connecting to server', detail: url, severity: 'info' })
    await new Promise((resolve) => setTimeout(resolve, connectionDelay))
    const result = connectionResult(url.trim(), code.trim())
    setConnection(result)
    connecting.current = false
    if (result.status === 'connected') onPauseChange(pauseBeforeConnection.current)
    addActivity({
      title: result.status === 'connected' ? 'Connected to server' : 'Connection failed',
      detail: result.message ?? result.url,
      severity: result.status === 'connected' ? 'success' : 'error'
    })
  }

  return { connection, serverUrl, registrationCode, setServerUrl, setRegistrationCode, connect }
}
