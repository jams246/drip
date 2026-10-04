import type { Change, MirrorEntry, Offer, Operation, PlannedRegion, RegionFingerprint, RegionPage, Root, RootRegistration } from '../Domain/models.js'

export interface SynchronizationRepository {
  registerRoot(deviceId: string, registration: RootRegistration): Root
  root(deviceId: string, rootId: string): Root
  rootIds(deviceId: string): string[]
  heads(rootId: string, after: string, limit: number): MirrorEntry[]
  operation(deviceId: string, rootId: string, operationId: string): Operation
  offer(root: Root, offer: Offer): Operation
  abort(operation: Operation): void
  addRegions(operation: Operation, regions: RegionFingerprint[]): void
  seal(operation: Operation): void
  ready(operationId: string): void
  uploaded(operationId: string, offset: number): void
  region(operationId: string, offset: number): PlannedRegion | undefined
  required(operationId: string, after: number, limit: number): RegionPage
  hasMissing(operationId: string): boolean
  prepare(operation: Operation): void
  finalize(operation: Operation): void
  publishing(): Operation[]
  offered(expiredOnly?: boolean): Operation[]
  fenceDevice(deviceId: string): void
  retire(rootId: string): void
  removeRoot(rootId: string): void
}

export interface MirrorStorage {
  assertPathCapacity(root: Root, change: Change): void
  seed(operation: Operation): Promise<void>
  upload(operation: Operation, region: PlannedRegion, bytes: Uint8Array): Promise<void>
  prepare(operation: Operation): Promise<void>
  publish(operation: Operation): Promise<void>
  discard(operation: Operation): Promise<void>
  removeRoot(deviceId: string, rootId: string): Promise<void>
}
