import type { Operation } from '../Domain/models.js'
import type { MirrorStorage, SynchronizationRepository } from './ports.js'
import type { RootLocks } from './root-locks.js'

export class SynchronizationWork {
  private readonly tasks = new Map<string, Promise<void>>()
  private readonly publications = new Set<string>()
  private readonly failures = new Map<string, unknown>()
  private closing = false

  constructor(
    private readonly repository: SynchronizationRepository,
    private readonly storage: MirrorStorage,
    private readonly locks: RootLocks
  ) {}

  has(operationId: string) {
    return this.tasks.has(operationId)
  }
  publishing(operationId: string) {
    return this.publications.has(operationId)
  }
  clearFailure(operationId: string) {
    this.failures.delete(operationId)
  }
  failure(operationId: string) {
    const failure = this.failures.get(operationId)
    this.failures.delete(operationId)
    return failure
  }

  schedule(operation: Operation, kind: 'seed' | 'publish') {
    if (this.closing || this.tasks.has(operation.operationId)) return
    this.failures.delete(operation.operationId)
    if (kind === 'publish') this.publications.add(operation.operationId)
    const task = this.locks.run(operation.rootId, async () => {
      const current = this.repository.operation(operation.deviceId, operation.rootId, operation.operationId)
      if (kind === 'seed') {
        if (current.status !== 'offered' || current.stageReady) return
        await this.storage.seed(current)
        this.repository.root(current.deviceId, current.rootId)
        this.repository.ready(current.operationId)
        return
      }
      if (current.status === 'offered') {
        await this.storage.prepare(current)
        this.repository.prepare(current)
      } else if (current.status !== 'publishing') return
      await this.storage.publish(current)
      this.repository.finalize(current)
    })
    this.tasks.set(operation.operationId, task)
    void task
      .catch((error: unknown) => this.failures.set(operation.operationId, error))
      .finally(() => {
        this.tasks.delete(operation.operationId)
        this.publications.delete(operation.operationId)
      })
  }

  async close() {
    this.closing = true
    await Promise.allSettled(this.tasks.values())
    await this.locks.settled()
  }
}
