import { SyncError } from './errors.js'
import type { Change, Root, RootRegistration } from './models.js'

interface NamespaceConflicts {
  ancestorFile: boolean
  descendants: boolean
  otherFile: boolean
}

export function assertRootIdentity(root: Root, registration: RootRegistration) {
  if (root.name !== registration.name || root.kind !== registration.kind) throw new SyncError('conflict', 'Root identity is already registered differently.')
}

export function assertRevision(current: number, expected: number) {
  if (current !== expected) throw new SyncError('revision_conflict', 'Operation base revision is stale.')
}

export function assertPublicationNamespace(root: Root, change: Change, conflicts: NamespaceConflicts) {
  if (change.kind === 'delete') return
  if (conflicts.ancestorFile) throw new SyncError('conflict', 'Delete the ancestor file before creating descendants.')
  if (conflicts.descendants) throw new SyncError('conflict', 'Delete descendants before replacing their directory.')
  if (root.kind === 'file' && conflicts.otherFile) throw new SyncError('conflict', 'A file root can contain one file.')
}
