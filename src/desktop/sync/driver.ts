import type { OperationReceipt, RootReceipt } from '../../protocol/sync'
import { recordSyncError } from './diagnostics'
import { enrollDevice, recoverEnrollment } from './enrollment'
import { SyncHttpError, type SyncTransport, createSyncTransport } from './http'
import { reconcileRoot } from './reconcile'
import { SyncStore } from './store'
import { synchronizeOperation } from './synchronize'
import { validateReceipt } from './upload'
import type { SyncRoot, SyncStatus } from './types'

const FIRST_RETRY_MS = 1000
const MAX_RETRY_MS = 30_000
const SYNC_BATCH = 16
const HTTP_CONFLICT = 409
const HTTP_NOT_FOUND = 404
const HTTP_UNAUTHORIZED = 401
const HTTP_FORBIDDEN = 403
const RETRY_JITTER_MIN = 0.75

interface SyncDriverOptions {
  databasePath: string
  transport?: SyncTransport
  publish?: (status: SyncStatus) => void
  rescan?: (watchId: string, path: string) => void
  busy?: (watchId: string) => boolean
  allowLoopbackHttp?: boolean
}

export function createSyncDriver(options: SyncDriverOptions) {
  const store = new SyncStore(options.databasePath)
  const transport = options.transport ?? createSyncTransport()
  let stopping = false
  let work: Promise<void> | undefined
  let retryAt = 0
  let attempts = 0
  let authenticationFailed = false
  let reconciled = ''
  let transferredBytes = 0
  let current: SyncStatus = {
    state: 'disconnected',
    url: store.credentials()?.url ?? '',
    pending: store.pendingCount(),
    message: 'Connect to a server to start synchronization.',
    transferredBytes
  }

  function publish(state: SyncStatus['state'], message: string) {
    const next = { state, message, url: store.credentials()?.url ?? '', pending: store.pendingCount(), transferredBytes }
    if (JSON.stringify(current) === JSON.stringify(next)) return
    current = next
    options.publish?.(current)
  }
  function active(root: SyncRoot) {
    return !stopping && store.isActive(root.rootId)
  }
  function rescan(path: string) {
    for (const id of store.watchIds(path)) options.rescan?.(id, path)
  }

  // oxlint-disable-next-line eslint/max-statements -- One namespace owns registration, durable operation recovery, and safe reconciliation.
  async function driveRoot(root: SyncRoot) {
    const credentials = store.credentials()!
    if (!root.registered) {
      const value = await transport.request<RootReceipt>(credentials, '/v1/roots', 'POST', { rootId: root.rootId, name: root.name, kind: 'folder' })
      if (value.rootId !== root.rootId || !Number.isSafeInteger(value.revision) || value.revision < 0) throw new Error('Invalid mirror receipt.')
      store.registered(root.rootId, value.revision)
      root = { ...root, registered: true, revision: value.revision }
    }
    const saved = store.operation()
    const operation = saved ?? store.freeze(root.rootId)
    if (operation) {
      publish('uploading', 'Synchronizing mirror entries.')
      try {
        return await synchronizeOperation({
          store,
          transport,
          credentials,
          operation,
          fresh: !saved,
          active: () => active(root) && store.members(operation).every((member) => store.operationScopes(member).some((scope) => !options.busy?.(scope.id))),
          rescan,
          transferred: (bytes) => {
            transferredBytes += bytes
            publish('uploading', 'Synchronizing file changes.')
          }
        })
      } catch (error) {
        if (!(error instanceof SyncHttpError) || error.status !== HTTP_CONFLICT) throw error
        const endpoint = `/v1/roots/${operation.rootId}/operations/${operation.operationId}`
        try {
          const receipt = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint))
          if (receipt.status === 'committed') {
            store.acknowledge(operation, receipt.revision)
            return true
          }
          if (receipt.status === 'publishing') return false
          if (receipt.status === 'offered') await transport.request(credentials, endpoint, 'DELETE')
        } catch (statusError) {
          if (!(statusError instanceof SyncHttpError) || statusError.status !== HTTP_NOT_FOUND) throw statusError
        }
        if (operation.change.kind === 'move') store.disableMove(operation)
        const value = await transport.request<RootReceipt>(credentials, '/v1/roots', 'POST', { rootId: root.rootId, name: root.name, kind: 'folder' })
        store.discard(operation)
        store.revision(root.rootId, value.revision)
        reconciled = ''
        if (store.readyScopes().length) await reconcileRoot(store, transport, credentials, { ...root, revision: value.revision }, () => active(root))
        return false
      }
    }
    const scopes = store.readyScopes().filter((scope) => !options.busy?.(scope.id))
    const stamp = JSON.stringify([root.revision, scopes.map((scope) => [scope.id, scope.coverage])])
    if (active(root) && scopes.length && reconciled !== stamp) {
      await reconcileRoot(store, transport, credentials, root, () => active(root))
      reconciled = stamp
    }
    return false
  }

  async function run() {
    if (stopping || authenticationFailed || Date.now() < retryAt) return
    let credentials = store.credentials()
    if (!credentials) return
    try {
      if (!credentials.confirmed) {
        publish('connecting', 'Registering this computer.')
        credentials = await recoverEnrollment(store, transport)
      }
      if (!credentials) return
      for (let count = 0; count < SYNC_BATCH; count++) {
        if (stopping) break
        const root = store.roots()[0]
        if (!root || !(await driveRoot(root))) break
      }
      attempts = 0
      if (!stopping)
        publish(store.pendingCount() ? 'uploading' : 'idle', store.pendingCount() ? 'Synchronizing mirror entries.' : 'All pending changes synchronized.')
    } catch (error) {
      if (stopping) return
      recordSyncError('sync.pass.error', error)
      const delay = Math.min(MAX_RETRY_MS, FIRST_RETRY_MS * 2 ** attempts++)
      retryAt = Date.now() + Math.min(MAX_RETRY_MS, delay * (RETRY_JITTER_MIN + Math.random() / 2))
      authenticationFailed = error instanceof SyncHttpError && (error.status === HTTP_UNAUTHORIZED || error.status === HTTP_FORBIDDEN)
      publish(authenticationFailed ? 'error' : 'retrying', error instanceof Error ? error.message : String(error))
    }
  }
  function tick() {
    if (stopping) return Promise.resolve()
    if (work) return work
    work = run().finally(() => {
      work = undefined
    })
    return work
  }
  async function connect(url: string, token: string, name?: string) {
    if (stopping) return
    if (work) await work
    authenticationFailed = false
    publish('connecting', 'Connecting to your server.')
    work = (async () => {
      try {
        await enrollDevice(store, transport, url, token, name, options.allowLoopbackHttp)
        attempts = 0
        retryAt = 0
        reconciled = ''
        transferredBytes = 0
        publish('idle', 'Computer registered. Synchronization is ready.')
      } catch (error) {
        recordSyncError('sync.connect.error', error)
        publish('error', error instanceof Error ? error.message : String(error))
      }
    })().finally(() => {
      work = undefined
    })
    await work
  }
  async function stop() {
    if (stopping) return
    stopping = true
    transport.cancel()
    if (work) await work
    store.close()
  }
  options.publish?.(current)
  return { tick, connect, stop, status: () => current, store }
}
