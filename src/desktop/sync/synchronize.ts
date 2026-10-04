import type { OperationReceipt } from '../../protocol/sync'
import { SyncHttpError, type SyncTransport } from './http'
import type { SyncStore } from './store'
import type { FrozenOperation, SyncCredentials } from './types'
import { validateOperationPaths } from './validation'
import { SourceChangedError, uploadMissing, validateMetadata, validateReceipt, validateSource } from './upload'
const HTTP_NOT_FOUND = 404
const STAGE_POLL_MS = 100

interface OperationContext {
  store: SyncStore
  transport: SyncTransport
  credentials: SyncCredentials
  operation: FrozenOperation
  fresh: boolean
  active: () => boolean
  transferred: (bytes: number) => void
  rescan: (path: string) => void
}

function endpoint(operation: FrozenOperation) {
  return `/v1/roots/${operation.rootId}/operations/${operation.operationId}`
}

// oxlint-disable-next-line eslint/max-statements -- Durable operation recovery and publication share one receipt lifecycle.
export async function synchronizeOperation(context: OperationContext) {
  const { store, transport, credentials, operation } = context
  const path = endpoint(operation)
  async function receipt() {
    try {
      return validateReceipt(await transport.request<OperationReceipt>(credentials, path))
    } catch (error) {
      if (!(error instanceof SyncHttpError) || error.status !== HTTP_NOT_FOUND) throw error
      if (operation.abortRequested)
        return { status: 'aborted', revision: operation.baseRevision, planComplete: false, stageReady: false, uploadRequired: false } as OperationReceipt
      return validateReceipt(
        await transport.request<OperationReceipt>(credentials, `/v1/roots/${operation.rootId}/operations`, 'POST', {
          operationId: operation.operationId,
          baseRevision: operation.baseRevision,
          change: operation.change
        })
      )
    }
  }
  function finish(value: OperationReceipt) {
    if (value.status === 'committed') {
      store.acknowledge(operation, value.revision)
      return true
    }
    if (value.status === 'aborted') {
      store.discard(operation, operation.abortRequested)
      return true
    }
    return false
  }
  async function abort(removePending = false) {
    store.requestAbort(operation)
    const value = validateReceipt(await transport.request<OperationReceipt>(credentials, path, 'DELETE'))
    if (value.status === 'committed') return finish(value)
    if (value.status !== 'aborted') return false
    store.discard(operation, removePending)
    return true
  }
  const proofPaths = new Set<string>()
  function rescanMembers() {
    const paths = new Set(proofPaths)
    for (const member of store.members(operation)) {
      if (member.sourcePath) paths.add(member.sourcePath)
      else for (const scope of store.operationScopes(member)) paths.add(scope.proofPath || scope.path)
    }
    for (const source of paths) context.rescan(source)
  }
  const initialChecks: (() => void)[] = []
  if (context.fresh) {
    try {
      if (!context.active() || !store.current(operation)) throw new SourceChangedError('Source changed before synchronization.')
      initialChecks.push(...validateOperationPaths(store, operation, proofPaths))
    } catch (error) {
      if (!(error instanceof SourceChangedError)) throw error
      rescanMembers()
      store.discard(operation)
      return true
    }
  }
  const value = await receipt()
  if (finish(value)) return true
  if (value.status === 'publishing') return false
  if (operation.abortRequested || !context.active()) return abort(operation.abortRequested)
  if (!context.fresh) return abort()
  const active = () => context.active() && store.current(operation)
  try {
    if (!active()) throw new SourceChangedError('Source changed before synchronization.')
    const checks = [...initialChecks, ...validateOperationPaths(store, operation, proofPaths)]
    for (const member of store.members(operation)) {
      if (member.change.kind !== 'upsert' || member.change.entry.kind !== 'file') continue
      const entry = member.change.entry
      const createdMs = validateMetadata(member.sourcePath, entry)
      checks.push(() => {
        validateMetadata(member.sourcePath, entry, createdMs)
      })
    }
    await uploadMissing(transport, credentials, store, operation, value, active, context.transferred)
    if (operation.change.kind === 'move') {
      for (const member of store.members(operation)) {
        if (member.change.kind === 'upsert' && member.change.entry.kind === 'file') await validateSource(member.sourcePath, member.change.entry, active)
      }
    }
    let ready = validateReceipt(await transport.request<OperationReceipt>(credentials, path))
    while (!ready.stageReady && ready.status === 'offered') {
      if (!active()) throw new SourceChangedError('Source changed before commit.')
      await new Promise<void>((resolve) => setTimeout(resolve, STAGE_POLL_MS))
      ready = validateReceipt(await transport.request<OperationReceipt>(credentials, path))
    }
    if (finish(ready)) return true
    if (!active()) throw new SourceChangedError('Source changed before commit.')
    validateOperationPaths(store, operation)
    for (const check of checks) check()
    return finish(validateReceipt(await transport.request<OperationReceipt>(credentials, path + '/commit', 'POST')))
  } catch (error) {
    if (!(error instanceof SourceChangedError)) throw error
    rescanMembers()
    return abort()
  }
}
