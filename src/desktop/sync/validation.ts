import { lstatSync, readdirSync } from 'node:fs'
import { inspectScopedScanPath } from '../scan/eligibility'
import { changePath } from '../storage/sync-outbox'
import type { SyncStore } from './store'
import type { FrozenOperation } from './types'
import { SourceChangedError } from './upload'

export function validateOperationPaths(store: SyncStore, operation: FrozenOperation, proofPaths?: Set<string>) {
  const checks: (() => void)[] = []
  for (const member of store.members(operation)) {
    const path = member.sourcePath || changePath(member.change)
    const deleting = member.change.kind === 'delete'
    const eligible = store.operationScopes(member).some((scope) => {
      try {
        if (inspectScopedScanPath(scope.path, scope.path, store.databasePath) !== 'eligible') return false
        const stats = lstatSync(scope.path)
        if (scope.kind === 'folder') {
          if (!stats.isDirectory() || stats.isSymbolicLink()) return false
          readdirSync(scope.path)
        } else if (!stats.isFile()) return false
        const proof = () => {
          if (
            scope.proofPath &&
            (inspectScopedScanPath(scope.proofPath, scope.path, store.databasePath) !== 'eligible' || !lstatSync(scope.proofPath).isFile())
          )
            throw new SourceChangedError('A source file proving absence changed before commit.')
        }
        proof()
        if (inspectScopedScanPath(path, scope.path, store.databasePath) !== (deleting ? 'missing' : 'eligible')) return false
        if (scope.proofPath) {
          checks.push(proof)
          proofPaths?.add(scope.proofPath)
        }
        return true
      } catch {
        return false
      }
    })
    if (!eligible) throw new SourceChangedError('Source or selected location changed before commit.')
    if (member.change.kind !== 'upsert' || member.change.entry.kind !== 'directory') continue
    try {
      const stats = lstatSync(path)
      if (!stats.isDirectory() || stats.isSymbolicLink()) throw new SourceChangedError('Source directory changed before commit.')
      readdirSync(path)
    } catch (error) {
      if (error && typeof error === 'object' && 'code' in error && (error.code === 'ENOENT' || error.code === 'ENOTDIR')) {
        throw new SourceChangedError('Source directory disappeared before commit.')
      }
      throw new SourceChangedError('Source directory became inaccessible before commit.')
    }
  }
  return checks
}
