import type { ApiError } from '../../protocol/sync'
import { recordDiagnostic } from '../diagnostics'
import type { SyncCredentials } from './types'
const HTTP_NO_CONTENT = 204
const HTTP_TIMEOUT_MS = 30_000

export class SyncHttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string
  ) {
    super(message)
  }
}

export interface SyncTransport {
  request<T>(credentials: SyncCredentials, path: string, method?: string, body?: unknown, binaryLength?: number): Promise<T>
  cancel(): void
}

export function normalizeServerUrl(value: string, allowLoopbackHttp = false): string {
  const url = new URL(value.trim())
  const loopback = url.hostname === 'localhost' || url.hostname === '127.0.0.1' || url.hostname === '[::1]'
  if (url.protocol !== 'https:' && !(allowLoopbackHttp && loopback && url.protocol === 'http:')) throw new Error('Use an HTTPS server URL.')
  if (url.username || url.password || url.search || url.hash || (url.pathname !== '/' && url.pathname !== '')) {
    throw new Error('Enter the server origin without credentials, query, or path.')
  }
  return url.origin
}

function requestRoute(path: string): string {
  if (path === '/v1/enroll') return 'enroll'
  if (path === '/v1/device') return 'device'
  if (path === '/v1/roots') return 'root'
  if (!path.startsWith('/v1/roots/')) return 'unknown'
  if (path.includes('/regions')) return 'region'
  if (path.endsWith('/commit')) return 'commit'
  if (path.includes('/operations')) return 'operation'
  if (path.includes('/heads')) return 'heads'
  return 'unknown'
}

async function readResponse(response: Response, details: string): Promise<unknown> {
  recordDiagnostic('sync.http.body.start', details)
  const text = await response.text()
  recordDiagnostic('sync.http.body.result', details)
  const value: unknown = JSON.parse(text)
  recordDiagnostic('sync.http.decode.result', details)
  return value
}

export function createSyncTransport(): SyncTransport {
  const controllers = new Set<AbortController>()
  let stopped = false
  return {
    // oxlint-disable-next-line eslint/max-statements -- One request owns payload, response diagnostics, error translation, and timeout cleanup.
    async request<T>(credentials: SyncCredentials, path: string, method = 'GET', body?: unknown, binaryLength?: number): Promise<T> {
      if (stopped) throw new Error('Synchronization transport has stopped.')
      const controller = new AbortController()
      controllers.add(controller)
      const timer = setTimeout(() => controller.abort(), HTTP_TIMEOUT_MS)
      try {
        const headers: Record<string, string> = { Authorization: `Bearer ${credentials.deviceId}.${credentials.secret}` }
        if (body !== undefined) headers['Content-Type'] = body instanceof Uint8Array ? 'application/octet-stream' : 'application/json'
        let payload: BodyInit | undefined
        if (body instanceof Uint8Array) {
          if (binaryLength !== undefined && (!Number.isSafeInteger(binaryLength) || binaryLength < 0 || binaryLength > body.length))
            throw new RangeError('Invalid binary request length.')
          payload = binaryLength === undefined || binaryLength === body.length ? new Uint8Array(body) : body.slice(0, binaryLength)
        } else if (body !== undefined) payload = JSON.stringify(body)
        const details = `route=${requestRoute(path)} method=${method}`
        recordDiagnostic('sync.http.fetch.start', details)
        // oxlint-disable-next-line unicorn/no-invalid-fetch-options -- The protocol method is dynamic; Perry requires an inline options object.
        const response = await fetch(credentials.url + path, { method: method, headers: headers, redirect: 'error', signal: controller.signal, body: payload })
        recordDiagnostic('sync.http.fetch.result', `${details} status=${response.status}`)
        if (!response.ok) {
          let error: ApiError = { code: 'http_error', message: `Server request failed (${response.status}).` }
          try {
            const value = await readResponse(response, details)
            if (
              value &&
              typeof value === 'object' &&
              'code' in value &&
              'message' in value &&
              typeof value.code === 'string' &&
              typeof value.message === 'string'
            ) {
              error = { code: value.code, message: value.message }
            }
          } catch {
            /* Keep the HTTP failure when the error response is incomplete. */
          }
          throw new SyncHttpError(response.status, error.code, error.message)
        }
        // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- Binary PUT callers expect no response body.
        if (response.status === HTTP_NO_CONTENT) return undefined as T
        // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- Protocol-specific callers validate the decoded response.
        return (await readResponse(response, details)) as T
      } finally {
        clearTimeout(timer)
        controllers.delete(controller)
      }
    },
    cancel() {
      stopped = true
      for (const controller of controllers) controller.abort()
    }
  }
}
