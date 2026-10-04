import { SyncError } from '../Domain/errors.js'
import type { HeadsPage, Operation, OperationReceipt } from '../Domain/models.js'
import { nonnegativeInteger, parseOffer, parseRootRegistration, validateHash, validateId } from '../Domain/validation.js'
import type { MirrorStorage, SynchronizationRepository } from './ports.js'
import { RootLocks } from './root-locks.js'

const DEFAULT_PAGE_LIMIT = 100
const MAX_PAGE_LIMIT = 1000

export interface PurgeDeviceMirrors {
  purgeDeviceMirrors(deviceId: string): Promise<void>
}

export class SynchronizationApplication implements PurgeDeviceMirrors {
  private readonly locks = new RootLocks()
  private readonly publications = new Map<string, Promise<void>>()
  private readonly failures = new Map<string, unknown>()
  private readonly uploads = new Map<string, Set<Promise<void>>>()

  constructor(
    private readonly repository: SynchronizationRepository,
    private readonly storage: MirrorStorage
  ) {}

  registerRoot(deviceId: string, value: unknown) {
    const root = this.repository.registerRoot(validateId(deviceId), parseRootRegistration(value))
    return { rootId: root.rootId, revision: root.revision }
  }

  listHeads(deviceId: string, rootId: string, query: { revision?: number; after?: string; limit?: number } = {}): HeadsPage {
    const root = this.repository.root(validateId(deviceId), validateId(rootId))
    if (query.revision !== undefined && nonnegativeInteger(query.revision, 'revision') !== root.revision) {
      throw new SyncError('revision_conflict', 'Root revision changed. Restart the inventory.')
    }
    const limit = nonnegativeInteger(query.limit ?? DEFAULT_PAGE_LIMIT, 'page limit')
    if (!limit || limit > MAX_PAGE_LIMIT || typeof (query.after ?? '') !== 'string') throw new SyncError('invalid_request', 'Invalid inventory page.')
    const heads = this.repository.heads(rootId, query.after ?? '', limit + 1)
    const hasNext = heads.length > limit
    const page = heads.slice(0, limit)
    return { revision: root.revision, heads: page, next: hasNext ? page.at(-1)!.path.toLowerCase() : null }
  }

  async offer(deviceId: string, rootId: string, value: unknown): Promise<OperationReceipt> {
    const offer = parseOffer(value)
    return this.locks.run(validateId(rootId), async () => {
      const root = this.repository.root(validateId(deviceId), rootId)
      this.storage.assertPathCapacity(root, offer.change)
      this.repository.expire()
      return this.receipt(this.repository.offer(root, offer))
    })
  }

  async uploadChunk(deviceId: string, rootId: string, operationId: string, hash: string, bytes: Uint8Array) {
    const operation = this.repository.operation(validateId(deviceId), validateId(rootId), validateId(operationId))
    validateHash(hash)
    if (operation.status !== 'offered') throw new SyncError('conflict', 'Operation is not accepting chunks.')
    if (operation.change.kind !== 'upsert' || !operation.change.chunks.some((chunk) => chunk.hash === hash)) {
      throw new SyncError('invalid_request', 'Chunk is not part of this operation.')
    }
    const upload = this.storage.upload(operation, hash, bytes)
    const uploads = this.uploads.get(rootId) ?? new Set<Promise<void>>()
    uploads.add(upload)
    this.uploads.set(rootId, uploads)
    try {
      await upload
      this.repository.root(deviceId, rootId)
    } finally {
      uploads.delete(upload)
      if (!uploads.size) this.uploads.delete(rootId)
    }
  }

  async commit(deviceId: string, rootId: string, operationId: string): Promise<OperationReceipt> {
    return this.locks.run(validateId(rootId), async () => {
      const operation = this.repository.operation(validateId(deviceId), rootId, validateId(operationId))
      if (operation.status === 'committed') return this.receipt(operation)
      if (operation.status === 'aborted') throw new SyncError('conflict', 'Operation has been aborted.')
      if (this.repository.missing(operation).length) throw new SyncError('chunks_missing', 'Upload missing chunks before committing.')
      this.repository.prepare(operation)
      this.schedule(operation)
      return this.receipt(this.repository.operation(deviceId, rootId, operationId))
    })
  }

  status(deviceId: string, rootId: string, operationId: string): OperationReceipt {
    const operation = this.repository.operation(validateId(deviceId), validateId(rootId), validateId(operationId))
    const failure = this.failures.get(operationId)
    if (operation.status === 'publishing') this.schedule(operation)
    if (failure) throw new SyncError('storage_failure', failure instanceof Error ? failure.message : 'Storage publication requires retry.')
    return this.receipt(operation)
  }

  async abort(deviceId: string, rootId: string, operationId: string): Promise<OperationReceipt> {
    return this.locks.run(validateId(rootId), async () => {
      const operation = this.repository.operation(validateId(deviceId), rootId, validateId(operationId))
      if (operation.status === 'publishing' || operation.status === 'committed') throw new SyncError('conflict', 'Publication cannot be aborted.')
      this.repository.abort(operation)
      return this.receipt(this.repository.operation(deviceId, rootId, operationId))
    })
  }

  async purgeDeviceMirrors(deviceId: string) {
    validateId(deviceId)
    this.repository.fenceDevice(deviceId)
    for (const rootId of this.repository.rootIds(deviceId)) {
      const uploads = this.uploads.get(rootId)
      if (uploads) await Promise.allSettled(uploads)
      await this.locks.run(rootId, async () => {
        this.repository.retire(rootId)
        await this.storage.removeRoot(deviceId, rootId)
        this.repository.removeRoot(rootId)
      })
    }
  }

  async recover() {
    this.repository.expire()
    for (const operation of this.repository.publishing()) {
      await this.locks.run(operation.rootId, async () => {
        this.repository.root(operation.deviceId, operation.rootId)
        await this.storage.publish(operation)
        this.repository.finalize(operation)
      })
    }
  }

  async close() {
    for (const uploads of this.uploads.values()) await Promise.allSettled(uploads)
    await Promise.allSettled(this.publications.values())
    await this.locks.settled()
  }

  private schedule(operation: Operation) {
    if (this.publications.has(operation.operationId)) return
    this.failures.delete(operation.operationId)
    const publication = this.locks.run(operation.rootId, async () => {
      this.repository.root(operation.deviceId, operation.rootId)
      await this.storage.publish(operation)
      this.repository.finalize(operation)
    })
    this.publications.set(operation.operationId, publication)
    void publication.catch((error: unknown) => this.failures.set(operation.operationId, error)).finally(() => this.publications.delete(operation.operationId))
  }

  private receipt(operation: Operation): OperationReceipt {
    return { status: operation.status, revision: operation.revision, missing: operation.status === 'offered' ? this.repository.missing(operation) : [] }
  }
}
