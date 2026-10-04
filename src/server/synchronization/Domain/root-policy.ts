import { SyncError } from './errors.js'
import type { Root, RootRegistration } from './models.js'

export function assertRootIdentity(root: Root, registration: RootRegistration) {
  if (root.name !== registration.name || root.kind !== registration.kind) throw new SyncError('conflict', 'Root identity is already registered differently.')
}

export function assertRevision(current: number, expected: number) {
  if (current !== expected) throw new SyncError('revision_conflict', 'Operation base revision is stale.')
}
