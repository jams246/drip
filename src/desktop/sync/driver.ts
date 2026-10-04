import type { OperationReceipt, RootReceipt } from '../../protocol/sync'
import { recordDiagnostic } from '../diagnostics'
import { recordSyncError } from './diagnostics'
import { enrollDevice, recoverEnrollment } from './enrollment'
import { SyncHttpError, type SyncTransport, createSyncTransport } from './http'
import { reconcileRoot } from './reconcile'
import { SyncStore } from './store'
import { SourceChangedError, uploadMissing, validateReceipt } from './upload'
import type { FrozenOperation, SyncCredentials, SyncRoot, SyncStatus } from './types'
const HTTP_NOT_FOUND = 404
const HTTP_CONFLICT = 409
const HTTP_UNAUTHORIZED = 401
const HTTP_FORBIDDEN = 403
const SYNC_FIRST_RETRY_MS = 1000
const SYNC_MAX_RETRY_MS = 30_000
const SYNC_RETRY_JITTER_MIN = 0.75
const SYNC_ROOT_BATCH = 16

function endpoint(operation: FrozenOperation) {
  return `/v1/roots/${operation.rootId}/operations/${operation.operationId}`
}

interface SyncDriverOptions {
  databasePath: string
  transport?: SyncTransport
  publish?: (status: SyncStatus) => void
  rescan?: (watchId: string, path: string) => void
  busy?: (watchId: string) => boolean
  allowLoopbackHttp?: boolean
}

// oxlint-disable-next-line eslint/max-statements -- Driver owns durable operations, retry scheduling, and shutdown callbacks.
export function createSyncDriver(options: SyncDriverOptions) {
  const store = new SyncStore(options.databasePath)
  const transport = options.transport ?? createSyncTransport()
  const reconciled = new Map<string, number>()
  let stopping = false
  let work: Promise<void> | undefined
  let retryAt = 0
  let attempts = 0
  let authenticationFailed = false
  let current: SyncStatus = {
    state: 'disconnected',
    url: store.credentials()?.url ?? '',
    pending: store.pendingCount(),
    message: 'Connect to a server to start synchronization.'
  }

  function publish(state: SyncStatus['state'], message: string) {
    const next = { state, message, url: store.credentials()?.url ?? '', pending: store.pendingCount() }
    if (current.state === next.state && current.message === next.message && current.url === next.url && current.pending === next.pending) return
    current = next
    options.publish?.(current)
  }
  function active(root: SyncRoot) {
    return !stopping && store.isActive(root.rootId)
  }
  async function receipt(credentials: SyncCredentials, operation: FrozenOperation) {
    recordDiagnostic('sync.receipt.start', `operation=${operation.operationId}`)
    try {
      return validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation)))
    } catch (error) {
      if (!(error instanceof SyncHttpError) || error.status !== HTTP_NOT_FOUND) throw error
      if (operation.abortRequested) return { status: 'aborted', revision: operation.baseRevision, missing: [] } as OperationReceipt
      recordDiagnostic('sync.operation.offer.start', `operation=${operation.operationId} kind=${operation.change.kind}`)
      return validateReceipt(
        await transport.request<OperationReceipt>(credentials, `/v1/roots/${operation.rootId}/operations`, 'POST', {
          operationId: operation.operationId,
          baseRevision: operation.baseRevision,
          change: operation.change
        })
      )
    }
  }
  async function finishReceipt(operation: FrozenOperation, value: OperationReceipt): Promise<boolean> {
    recordDiagnostic('sync.receipt.result', `operation=${operation.operationId} status=${value.status} missing=${value.missing.length}`)
    if (value.status === 'committed') {
      store.acknowledge(operation, value.revision)
      publish('idle', 'File synchronized.')
      return true
    }
    if (value.status === 'aborted') {
      store.discard(operation, operation.abortRequested)
      return true
    }
    return false
  }
  // oxlint-disable-next-line eslint/max-statements -- One frozen operation owns upload, publication, and source-change cancellation.
  async function synchronize(credentials: SyncCredentials, root: SyncRoot, operation: FrozenOperation) {
    const value = await receipt(credentials, operation)
    if (await finishReceipt(operation, value)) return true
    if (operation.abortRequested || !active(root)) {
      store.requestAbort(operation)
      const aborted = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation), 'DELETE'))
      return finishReceipt({ ...operation, abortRequested: true }, aborted)
    }
    if (value.status === 'publishing') return false
    publish('uploading', `Synchronizing ${operation.change.path}.`)
    let phase = 'upload'
    try {
      recordDiagnostic('sync.upload.start', `operation=${operation.operationId} missing=${value.missing.length}`)
      await uploadMissing(transport, credentials, operation, value, () => active(root))
      const continuing = active(root)
      recordDiagnostic('sync.upload.result', `operation=${operation.operationId} active=${continuing}`)
      if (!continuing) return false
      phase = 'commit'
      recordDiagnostic('sync.commit.start', `operation=${operation.operationId}`)
      let committed = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation) + '/commit', 'POST'))
      if (committed.status === 'publishing') committed = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation)))
      recordDiagnostic('sync.commit.result', `operation=${operation.operationId} status=${committed.status} revision=${committed.revision}`)
      return finishReceipt(operation, committed)
    } catch (error) {
      recordSyncError('sync.operation.error', error, `operation=${operation.operationId} phase=${phase}`)
      if (!(error instanceof SourceChangedError)) throw error
      store.requestAbort(operation)
      options.rescan?.(root.watchId, operation.sourcePath)
      const aborted = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation), 'DELETE'))
      await finishReceipt({ ...operation, abortRequested: true }, aborted)
      publish('retrying', 'Source file changed. Waiting for a fresh scan.')
      return false
    }
  }
  // oxlint-disable-next-line eslint/max-statements -- Resolve durable operations before creating successors or reconciling heads.
  async function driveRoot(credentials: SyncCredentials, root: SyncRoot) {
    if (!root.registered && active(root)) {
      recordDiagnostic('sync.root.register.start', `root=${root.rootId} kind=${root.kind}`)
      const value = await transport.request<RootReceipt>(credentials, '/v1/roots', 'POST', { rootId: root.rootId, name: root.name, kind: root.kind })
      if (value.rootId !== root.rootId || !Number.isSafeInteger(value.revision)) throw new Error('Invalid synchronization root receipt.')
      store.registered(root.rootId, value.revision)
      root = { ...root, registered: true, revision: value.revision }
      recordDiagnostic('sync.root.register.result', `root=${root.rootId} revision=${value.revision}`)
    }
    const operation = store.operation(root.rootId) ?? (active(root) && !options.busy?.(root.watchId) ? store.freeze(root.rootId) : undefined)
    if (operation) {
      try {
        return await synchronize(credentials, root, operation)
      } catch (error) {
        recordSyncError('sync.operation.failed', error, `operation=${operation.operationId}`)
        if (!(error instanceof SyncHttpError) || error.status !== HTTP_CONFLICT) throw error
        try {
          const existing = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation)))
          if (existing.status === 'offered' && error.code !== 'revision_conflict') {
            const aborted = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation), 'DELETE'))
            await finishReceipt(operation, aborted)
            return false
          }
          return finishReceipt(operation, existing)
        } catch (statusError) {
          if (!(statusError instanceof SyncHttpError) || statusError.status !== HTTP_NOT_FOUND) throw statusError
        }
        const value = await transport.request<RootReceipt>(credentials, '/v1/roots', 'POST', { rootId: root.rootId, name: root.name, kind: root.kind })
        store.discard(operation)
        store.revision(root.rootId, value.revision)
        reconciled.delete(root.rootId)
      }
      return false
    }
    if (active(root) && !options.busy?.(root.watchId) && store.safeCoverage(root) && reconciled.get(root.rootId) !== root.revision) {
      await reconcileRoot(store, transport, credentials, root, () => active(root) && !options.busy?.(root.watchId))
      if (store.safeCoverage(root) && !options.busy?.(root.watchId)) reconciled.set(root.rootId, root.revision)
    }
    return false
  }
  // oxlint-disable-next-line eslint/max-statements -- One pass owns authentication, bounded fair draining, and retry scheduling.
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
      for (const root of store.roots()) {
        if (stopping) break
        for (let count = 0; count < SYNC_ROOT_BATCH; count++) {
          if (stopping) break
          const latest = store.roots().find((entry) => entry.rootId === root.rootId)!
          if (!(await driveRoot(credentials, latest))) break
        }
      }
      attempts = 0
      if (!stopping) publish(store.pendingCount() ? 'uploading' : 'idle', store.pendingCount() ? 'Synchronizing files.' : 'All pending changes synchronized.')
    } catch (error) {
      if (stopping) return
      recordSyncError('sync.pass.error', error)
      const delay = Math.min(SYNC_MAX_RETRY_MS, SYNC_FIRST_RETRY_MS * 2 ** attempts++)
      retryAt = Date.now() + Math.min(SYNC_MAX_RETRY_MS, delay * (SYNC_RETRY_JITTER_MIN + Math.random() / 2))
      const rejected = error instanceof SyncHttpError && (error.status === HTTP_UNAUTHORIZED || error.status === HTTP_FORBIDDEN)
      authenticationFailed = rejected
      publish(rejected ? 'error' : 'retrying', error instanceof Error ? error.message : String(error))
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
    recordDiagnostic('sync.connect.start')
    publish('connecting', 'Connecting to your server.')
    work = (async () => {
      try {
        await enrollDevice(store, transport, url, token, name, options.allowLoopbackHttp)
        attempts = 0
        retryAt = 0
        reconciled.clear()
        recordDiagnostic('sync.connect.result', 'status=registered')
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
