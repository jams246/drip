import type { DatabaseSync } from 'node:sqlite'
import type { Change, MirrorEntry } from '../../protocol/sync'
import { movedFiles } from './moves'
import type { OperationMember } from './types'

export interface Pending extends OperationMember {
  previous?: MirrorEntry
}
export function pendingEntries(database: DatabaseSync): Pending[] {
  return database
    .prepare('SELECT * FROM sync_pending ORDER BY generation')
    .all()
    .map((row) => ({
      pathKey: String(row.path_key),
      generation: Number(row.generation),
      sourcePath: String(row.source_path),
      change: JSON.parse(String(row.change)),
      previous: row.previous ? JSON.parse(String(row.previous)) : undefined
    }))
}

function descendants(path: string, candidate: string) {
  return candidate.startsWith(path.endsWith('/') ? path : path + '/')
}

export function nextOperation(
  database: DatabaseSync,
  eligible: (member: OperationMember) => boolean
): { change: Change; members: OperationMember[] } | undefined {
  const pending = pendingEntries(database).filter(eligible)
  const protectedSources = new Set<string>()
  const moved = movedFiles(pending, protectedSources)
  if (moved) return moved
  const structural = pending.find(
    (row) =>
      row.change.kind === 'upsert' &&
      row.previous &&
      row.previous.kind !== row.change.entry.kind &&
      !pending.some((child) => child.change.kind === 'delete' && descendants(row.pathKey, child.pathKey))
  )
  if (structural?.previous) return { change: { kind: 'delete', path: structural.previous.path }, members: [structural] }
  const directory = pending
    .filter((row) => row.change.kind === 'upsert' && row.change.entry.kind === 'directory' && (!row.previous || row.previous.kind === 'directory'))
    .toSorted((left, right) => left.pathKey.length - right.pathKey.length)[0]
  if (directory) return { change: directory.change, members: [directory] }
  const deleted = pending
    .filter((row) => row.change.kind === 'delete' && !protectedSources.has(row.pathKey))
    .toSorted((left, right) => right.pathKey.length - left.pathKey.length)[0]
  const next = deleted ?? pending[0]
  return next ? { change: next.change, members: [next] } : undefined
}
