import { chmod, unlink } from 'node:fs/promises'
import { createConnection, createServer } from 'node:net'
import type { Socket } from 'node:net'
import { join } from 'node:path'
import type { AccessApplication } from '../access/Application/access.js'
import { errorResponse } from './error-filter.js'

export type AdminCommand = { action: 'token-create' | 'devices-list' | 'devices-remove'; deviceId?: string }

const ADMIN_TIMEOUT_MS = 5000
const MAX_REQUEST_BYTES = 4096
const MAX_RESPONSE_BYTES = 1_048_576
const PRIVATE_SOCKET_MODE = 0o600

function parseCommand(value: unknown): AdminCommand {
  if (!value || typeof value !== 'object' || !('action' in value)) throw new Error('An administrative command is required.')
  if (value.action === 'token-create' || value.action === 'devices-list') return { action: value.action }
  if (value.action === 'devices-remove' && 'deviceId' in value && typeof value.deviceId === 'string') {
    return { action: value.action, deviceId: value.deviceId }
  }
  throw new Error('Unknown administrative command.')
}

export async function executeAdmin(access: AccessApplication, command: AdminCommand): Promise<unknown> {
  if (command.action === 'token-create') return access.createToken()
  if (command.action === 'devices-list') return access.listDevices()
  if (command.action === 'devices-remove' && typeof command.deviceId === 'string') {
    await access.removeDevice(command.deviceId)
    return { removed: command.deviceId }
  }
  throw new Error('Unknown administrative command.')
}

export async function startAdminSocket(dataDirectory: string, access: AccessApplication): Promise<() => Promise<void>> {
  const path = join(dataDirectory, '.admin.sock')
  await unlink(path).catch((error: NodeJS.ErrnoException) => {
    if (error.code !== 'ENOENT') throw error
  })
  const sockets = new Set<Socket>()
  const active = new Set<Promise<void>>()
  const server = createServer((socket) => {
    sockets.add(socket)
    socket.on('error', () => socket.destroy())
    socket.once('close', () => sockets.delete(socket))
    socket.setTimeout(ADMIN_TIMEOUT_MS, () => socket.destroy())
    let request = ''
    let handled = false
    socket.on('data', (bytes: Buffer) => {
      if (handled) return
      request += bytes.toString('utf8')
      if (request.length > MAX_REQUEST_BYTES) {
        socket.destroy()
        return
      }
      if (!request.includes('\n')) return
      handled = true
      socket.setTimeout(0)
      const operation = (async () => {
        try {
          const command = parseCommand(JSON.parse(request.slice(0, request.indexOf('\n'))))
          const result = await executeAdmin(access, command)
          socket.end(JSON.stringify({ ok: true, result }) + '\n')
        } catch (error) {
          socket.end(JSON.stringify({ ok: false, ...errorResponse(error).body }) + '\n')
        }
      })()
      active.add(operation)
      void operation.finally(() => active.delete(operation))
    })
  })
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(path, () => resolve())
  })
  await chmod(path, PRIVATE_SOCKET_MODE)
  return async () => {
    const closed = new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())))
    await Promise.all(active)
    for (const socket of sockets) socket.destroy()
    await closed
    await unlink(path).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== 'ENOENT') throw error
    })
  }
}

export async function requestAdmin(dataDirectory: string, command: AdminCommand): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(join(dataDirectory, '.admin.sock'))
    socket.once('error', reject)
    socket.setTimeout(ADMIN_TIMEOUT_MS, () => {
      socket.destroy()
      reject(new Error('The administrative request timed out.'))
    })
    socket.once('connect', () => {
      socket.setTimeout(0)
      socket.write(JSON.stringify(command) + '\n')
    })
    let response = ''
    socket.on('data', (bytes: Buffer) => {
      response += bytes.toString('utf8')
      if (response.length > MAX_RESPONSE_BYTES) {
        socket.destroy()
        reject(new Error('Administrative response exceeds the limit.'))
        return
      }
      if (!response.includes('\n')) return
      socket.end()
      try {
        const parsed: unknown = JSON.parse(response)
        if (!parsed || typeof parsed !== 'object' || !('ok' in parsed)) throw new Error('Invalid administrative response.')
        if (parsed.ok === true && 'result' in parsed) resolve(parsed.result)
        else {
          const message = 'message' in parsed && typeof parsed.message === 'string' ? parsed.message : 'The administrative request failed.'
          reject(new Error(message))
        }
      } catch (error) {
        reject(error)
      }
    })
    socket.once('end', () => {
      if (!response.includes('\n')) reject(new Error('The administrative connection closed without a response.'))
    })
  })
}
