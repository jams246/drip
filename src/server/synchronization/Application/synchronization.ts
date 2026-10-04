import { REGION_PAGE_LIMIT } from '../../../protocol/sync.js'
import { SyncError } from '../Domain/errors.js'
import type { HeadsPage, Operation, OperationReceipt, RegionPage } from '../Domain/models.js'
import { nonnegativeInteger, parseOffer, parseRegions, parseRootRegistration, validateId } from '../Domain/validation.js'
import type { MirrorStorage, SynchronizationRepository } from './ports.js'
import { SynchronizationWork } from './background-work.js'
import { RootLocks } from './root-locks.js'

const DEFAULT_PAGE_LIMIT = 100

export interface PurgeDeviceMirrors {
  purgeDeviceMirrors(deviceId: string): Promise<void>
}

export class SynchronizationApplication implements PurgeDeviceMirrors {
  private readonly locks = new RootLocks()
  private readonly work: SynchronizationWork

  constructor(
    private readonly repository: SynchronizationRepository,
    private readonly storage: MirrorStorage
  ) {
    this.work = new SynchronizationWork(repository, storage, this.locks)
  }

  registerRoot(deviceId: string, value: unknown) {
    const root = this.repository.registerRoot(validateId(deviceId), parseRootRegistration(value))
    return { rootId: root.rootId, revision: root.revision }
  }

  listHeads(deviceId: string, rootId: string, query: { revision?: number; after?: string; limit?: number } = {}): HeadsPage {
    const root = this.repository.root(validateId(deviceId), validateId(rootId))
    if (query.revision !== undefined && nonnegativeInteger(query.revision, 'revision') !== root.revision)
      throw new SyncError('revision_conflict', 'Device revision changed. Restart the inventory.')
    const limit = pageLimit(query.limit ?? DEFAULT_PAGE_LIMIT)
    if (typeof (query.after ?? '') !== 'string') throw new SyncError('invalid_request', 'Invalid inventory page.')
    const heads = this.repository.heads(rootId, query.after ?? '', limit + 1)
    return { revision: root.revision, heads: heads.slice(0, limit), next: heads.length > limit ? heads[limit - 1].path.toLowerCase() : null }
  }

  async offer(deviceId: string, rootId: string, value: unknown): Promise<OperationReceipt> {
    const offer = parseOffer(value)
    return this.locks.run(validateId(rootId), async () => {
      const root = this.repository.root(validateId(deviceId), rootId)
      this.storage.assertPathCapacity(root, offer.change)
      return this.receipt(this.repository.offer(root, offer))
    })
  }

  async addRegions(deviceId: string, rootId: string, operationId: string, value: unknown): Promise<RegionPage> {
    const regions = parseRegions(value)
    return this.locks.run(validateId(rootId), async () => {
      const operation = this.operation(deviceId, rootId, operationId)
      this.repository.addRegions(operation, regions)
      const required = regions
        .filter((region) => {
          const planned = this.repository.region(operationId, region.offset)!
          return planned.basisOffset === null && !planned.uploaded
        })
        .map((region) => region.offset)
      return { required, next: null }
    })
  }

  async completeRegions(deviceId: string, rootId: string, operationId: string): Promise<OperationReceipt> {
    return this.locks.run(validateId(rootId), async () => {
      const operation = this.operation(deviceId, rootId, operationId)
      this.repository.seal(operation)
      const sealed = this.operation(deviceId, rootId, operationId)
      if (!sealed.stageReady) this.work.schedule(sealed, 'seed')
      return this.receipt(sealed)
    })
  }

  listRegions(deviceId: string, rootId: string, operationId: string, query: { after?: number; limit?: number } = {}): RegionPage {
    this.operation(deviceId, rootId, operationId)
    const after = query.after ?? -1
    if (!Number.isSafeInteger(after) || after < -1) throw new SyncError('invalid_request', 'Invalid region cursor.')
    return this.repository.required(operationId, after, pageLimit(query.limit ?? REGION_PAGE_LIMIT))
  }

  async uploadRegion(deviceId: string, rootId: string, operationId: string, offset: number, bytes: Uint8Array) {
    nonnegativeInteger(offset, 'region offset')
    const offered = this.operation(deviceId, rootId, operationId)
    if (!offered.stageReady || !offered.planComplete) throw new SyncError('conflict', 'File staging is not ready for uploads.')
    return this.locks.run(rootId, async () => {
      const operation = this.operation(deviceId, rootId, operationId)
      if (operation.status !== 'offered') throw new SyncError('conflict', 'Operation does not accept uploads.')
      const region = this.repository.region(operationId, offset)
      if (!region || region.basisOffset !== null) throw new SyncError('invalid_request', 'Region is not required by this operation.')
      await this.storage.upload(operation, region, bytes)
      this.repository.root(deviceId, rootId)
      this.repository.uploaded(operationId, offset)
    })
  }

  async commit(deviceId: string, rootId: string, operationId: string): Promise<OperationReceipt> {
    const current = this.operation(deviceId, rootId, operationId)
    if (this.work.publishing(operationId)) return this.receipt(current)
    if (!current.stageReady) throw new SyncError('regions_missing', 'Wait for file staging before committing.')
    return this.locks.run(rootId, async () => {
      const operation = this.operation(deviceId, rootId, operationId)
      if (operation.status === 'committed' || operation.status === 'publishing') return this.receipt(operation)
      if (operation.status === 'aborted') throw new SyncError('conflict', 'Operation has been aborted.')
      if (!operation.planComplete || this.repository.hasMissing(operationId))
        throw new SyncError('regions_missing', 'Complete the file plan and uploads before committing.')
      this.work.schedule(operation, 'publish')
      return this.receipt(this.operation(deviceId, rootId, operationId))
    })
  }

  status(deviceId: string, rootId: string, operationId: string): OperationReceipt {
    const operation = this.operation(deviceId, rootId, operationId)
    const failure = this.work.failure(operationId)
    if (operation.status === 'publishing') this.work.schedule(operation, 'publish')
    if (operation.status === 'offered' && operation.planComplete && !operation.stageReady) this.work.schedule(operation, 'seed')
    if (failure) throw new SyncError('storage_failure', failure instanceof Error ? failure.message : 'Mirror storage requires retry.')
    return this.receipt(operation)
  }

  async abort(deviceId: string, rootId: string, operationId: string): Promise<OperationReceipt> {
    return this.locks.run(validateId(rootId), async () => {
      const operation = this.operation(deviceId, rootId, operationId)
      if (operation.status === 'publishing' || operation.status === 'committed') throw new SyncError('conflict', 'Publication cannot be aborted.')
      this.repository.abort(operation)
      await this.storage.discard(operation)
      this.work.clearFailure(operationId)
      return this.receipt(this.operation(deviceId, rootId, operationId))
    })
  }

  async purgeDeviceMirrors(deviceId: string) {
    validateId(deviceId)
    this.repository.fenceDevice(deviceId)
    for (const rootId of this.repository.rootIds(deviceId))
      await this.locks.run(rootId, async () => {
        this.repository.retire(rootId)
        await this.storage.removeRoot(deviceId, rootId)
        this.repository.removeRoot(rootId)
      })
  }

  async recover() {
    for (const operation of this.repository.publishing())
      await this.locks.run(operation.rootId, async () => {
        await this.storage.publish(operation)
        this.repository.finalize(operation)
      })
    for (const operation of this.repository.offered()) {
      this.repository.abort(operation)
      await this.storage.discard(operation)
    }
  }

  async expire() {
    for (const operation of this.repository.offered(true)) {
      if (this.work.has(operation.operationId)) continue
      await this.locks.run(operation.rootId, async () => {
        const current = this.operation(operation.deviceId, operation.rootId, operation.operationId)
        if (current.status !== 'offered') return
        this.repository.abort(current)
        await this.storage.discard(current)
      })
    }
  }

  async close() {
    await this.work.close()
  }

  private operation(deviceId: string, rootId: string, operationId: string) {
    return this.repository.operation(validateId(deviceId), validateId(rootId), validateId(operationId))
  }

  private receipt(operation: Operation): OperationReceipt {
    const needsContent = operation.change.kind === 'upsert' && operation.change.entry.kind === 'file' && !operation.metadataOnly
    return {
      status: operation.status === 'offered' && this.work.publishing(operation.operationId) ? 'publishing' : operation.status,
      revision: operation.revision,
      planComplete: operation.planComplete,
      stageReady: operation.stageReady,
      uploadRequired: operation.status === 'offered' && needsContent && (!operation.planComplete || this.repository.hasMissing(operation.operationId))
    }
  }
}

function pageLimit(value: number) {
  const limit = nonnegativeInteger(value, 'page limit')
  if (!limit || limit > REGION_PAGE_LIMIT) throw new SyncError('invalid_request', 'Invalid page limit.')
  return limit
}
