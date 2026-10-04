import type { DatabaseSync } from 'node:sqlite'
import type { RegionFingerprint } from '../../protocol/sync'
import { transaction } from '../storage/database'
import { randomId } from './identity'
import { nextOperation } from './pending'
import type { FrozenOperation, OperationMember } from './types'

export function readOperation(database: DatabaseSync): FrozenOperation | undefined {
  const row = database.prepare('SELECT * FROM sync_operations LIMIT 1').get()
  return row
    ? {
        rootId: String(row.root_id),
        operationId: String(row.operation_id),
        baseRevision: Number(row.base_revision),
        pathKey: String(row.path_key),
        generation: Number(row.generation),
        sourcePath: String(row.source_path),
        change: JSON.parse(String(row.change)),
        abortRequested: Boolean(row.abort_requested)
      }
    : undefined
}

export function operationMembers(database: DatabaseSync, operation: FrozenOperation): OperationMember[] {
  return database
    .prepare('SELECT * FROM sync_operation_members WHERE operation_id = ?')
    .all(operation.operationId)
    .map((row) => ({
      pathKey: String(row.path_key),
      generation: Number(row.generation),
      sourcePath: String(row.source_path),
      change: JSON.parse(String(row.change))
    }))
}

export function freezeOperation(database: DatabaseSync, rootId: string, eligible: (member: OperationMember) => boolean): FrozenOperation | undefined {
  transaction(database, () => {
    if (readOperation(database)) return
    const next = nextOperation(database, eligible)
    if (!next) return
    const state = database.prepare('SELECT root_id, registered, revision FROM sync_state WHERE id = 1').get()!
    if (state.root_id !== rootId || !state.registered) return
    const member = next.members[0]
    const operationId = randomId()
    database
      .prepare(`INSERT INTO sync_operations (root_id, operation_id, base_revision, path_key, generation, source_path, change)
      VALUES (?, ?, ?, ?, ?, ?, ?)`)
      .run(rootId, operationId, state.revision, member.pathKey, member.generation, member.sourcePath, JSON.stringify(next.change))
    for (const entry of next.members)
      database
        .prepare('INSERT INTO sync_operation_members VALUES (?, ?, ?, ?, ?)')
        .run(operationId, entry.pathKey, entry.generation, entry.sourcePath, JSON.stringify(entry.change))
    if (next.change.kind === 'upsert' && next.change.entry.kind === 'file') {
      const file = database.prepare('SELECT id FROM files WHERE path = ? AND hash = ? LIMIT 1').get(member.pathKey, next.change.entry.hash)
      if (!file) throw new Error('File fingerprints disappeared before synchronization.')
      database.prepare(`INSERT INTO frozen_regions SELECT ?, offset, length, hash FROM file_regions WHERE file_id = ?`).run(operationId, file.id)
    }
  })
  return readOperation(database)
}

export function readRegions(database: DatabaseSync, operation: FrozenOperation, after = -1, limit = 1000): RegionFingerprint[] {
  return database
    .prepare('SELECT offset, length, hash FROM frozen_regions WHERE operation_id = ? AND offset > ? ORDER BY offset LIMIT ?')
    .all(operation.operationId, after, limit)
    .map((row) => ({ offset: Number(row.offset), length: Number(row.length), hash: String(row.hash) }))
}

export function currentOperation(database: DatabaseSync, operation: FrozenOperation) {
  const saved = database.prepare('SELECT abort_requested FROM sync_operations WHERE operation_id = ?').get(operation.operationId)
  if (!saved || saved.abort_requested) return false
  return operationMembers(database, operation).every((member) => {
    const pending = database.prepare('SELECT generation FROM sync_pending WHERE path_key = ?').get(member.pathKey)
    return pending && Number(pending.generation) === member.generation
  })
}

function removeOperation(database: DatabaseSync, operation: FrozenOperation) {
  database.prepare('DELETE FROM frozen_regions WHERE operation_id = ?').run(operation.operationId)
  database.prepare('DELETE FROM sync_operation_members WHERE operation_id = ?').run(operation.operationId)
  database.prepare('DELETE FROM sync_operations WHERE operation_id = ?').run(operation.operationId)
}

export function acknowledgeOperation(database: DatabaseSync, operation: FrozenOperation, revision: number) {
  transaction(database, () => {
    database.prepare('UPDATE sync_state SET revision = ? WHERE id = 1').run(revision)
    const destinations = operation.change.kind === 'move' ? new Set(operation.change.moves.map((move) => move.entry.path.toLowerCase())) : undefined
    for (const member of operationMembers(database, operation)) {
      if (destinations && member.change.kind === 'upsert' && !destinations.has(member.pathKey)) continue
      if (operation.change.kind === 'delete' && member.change.kind === 'upsert') {
        database.prepare('UPDATE sync_pending SET previous = ? WHERE path_key = ?').run('', member.pathKey)
        continue
      }
      const previous = member.change.kind === 'upsert' ? JSON.stringify(member.change.entry) : ''
      database.prepare('UPDATE sync_pending SET previous = ? WHERE path_key = ? AND generation <> ?').run(previous, member.pathKey, member.generation)
      database.prepare('DELETE FROM sync_pending WHERE path_key = ? AND generation = ?').run(member.pathKey, member.generation)
    }
    if (operation.change.kind === 'move') {
      for (const move of operation.change.moves)
        if (!destinations!.has(move.from.toLowerCase())) {
          database.prepare('UPDATE sync_pending SET previous = ? WHERE path_key = ?').run('', move.from.toLowerCase())
        }
    }
    removeOperation(database, operation)
  })
}

export function discardOperation(database: DatabaseSync, operation: FrozenOperation, removePending = false) {
  transaction(database, () => {
    if (removePending)
      for (const member of operationMembers(database, operation)) {
        database.prepare('DELETE FROM sync_pending WHERE path_key = ? AND generation = ?').run(member.pathKey, member.generation)
      }
    removeOperation(database, operation)
  })
}

export function disableMove(database: DatabaseSync, operation: FrozenOperation) {
  for (const member of operationMembers(database, operation))
    database.prepare('UPDATE sync_pending SET previous = ? WHERE path_key = ?').run('', member.pathKey)
  if (operation.change.kind === 'move')
    for (const move of operation.change.moves) {
      database.prepare('UPDATE sync_pending SET previous = ? WHERE path_key = ?').run('', move.from.toLowerCase())
    }
}
