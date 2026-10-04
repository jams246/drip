import type { Change, FileHead, Offer, Operation, Root, RootRegistration } from '../Domain/models.js'

export interface SynchronizationRepository {
  assertDevice(deviceId: string): void
  registerRoot(deviceId: string, registration: RootRegistration): Root
  root(deviceId: string, rootId: string): Root
  rootIds(deviceId: string): string[]
  heads(rootId: string, after: string, limit: number): FileHead[]
  operation(deviceId: string, rootId: string, operationId: string): Operation
  offer(root: Root, offer: Offer): Operation
  abort(operation: Operation): void
  missing(operation: Operation): string[]
  prepare(operation: Operation): void
  finalize(operation: Operation): void
  publishing(): Operation[]
  expire(): void
  fenceDevice(deviceId: string): void
  retire(rootId: string): void
  removeRoot(rootId: string): void
}

export interface MirrorStorage {
  assertPathCapacity(root: Root, change: Change): void
  upload(operation: Operation, hash: string, bytes: Uint8Array): Promise<void>
  publish(operation: Operation): Promise<void>
  removeRoot(deviceId: string, rootId: string): Promise<void>
}
