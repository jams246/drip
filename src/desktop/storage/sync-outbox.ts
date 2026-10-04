import type { DatabaseSync } from 'node:sqlite'
import type { Change, MirrorEntry } from '../../protocol/sync'
const DRIVE_ROOT_LENGTH = 3

export function mirrorPath(path: string) {
  const normalized = path.split('\\').join('/')
  if (normalized.length === DRIVE_ROOT_LENGTH) return normalized.slice(0, 1).toUpperCase() + normalized.slice(1)
  return normalized.slice(0, 1).toUpperCase() + normalized.slice(1).replace(/\/$/, '')
}

export function changePath(change: Change): string {
  if (change.kind === 'upsert') return change.entry.path
  if (change.kind === 'delete') return change.path
  return change.moves[0].entry.path
}

export function coversPath(scope: { path: string; kind: unknown }, path: string) {
  const root = mirrorPath(scope.path).toLowerCase()
  const target = mirrorPath(path).toLowerCase()
  return target === root || (scope.kind === 'folder' && target.startsWith(root.endsWith('/') ? root : root + '/'))
}

export function enqueueChange(database: DatabaseSync, sourcePath: string, change: Change, previous?: MirrorEntry) {
  const pathKey = changePath(change).toLowerCase()
  const serialized = JSON.stringify(change)
  const pending = database.prepare('SELECT change, source_path, previous FROM sync_pending WHERE path_key = ?').get(pathKey)
  if (pending?.change === serialized && pending.source_path === sourcePath) {
    if (previous && !pending.previous) database.prepare('UPDATE sync_pending SET previous = ? WHERE path_key = ?').run(JSON.stringify(previous), pathKey)
    return
  }
  database.prepare('UPDATE sync_state SET generation = generation + 1 WHERE id = 1').run()
  const state = database.prepare('SELECT generation, root_id FROM sync_state WHERE id = 1').get()!
  database
    .prepare(`INSERT INTO sync_pending (root_id, path_key, generation, source_path, change, previous) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(path_key) DO UPDATE SET root_id = excluded.root_id, generation = excluded.generation,
    source_path = excluded.source_path, change = excluded.change,
    previous = CASE WHEN sync_pending.previous <> '' THEN sync_pending.previous ELSE excluded.previous END`)
    .run(String(state.root_id), pathKey, Number(state.generation), sourcePath, serialized, previous ? JSON.stringify(previous) : '')
}

export function catalogueEntry(database: DatabaseSync, watchId: string, path: string): { sourcePath: string; change: Change & { kind: 'upsert' } } | undefined {
  const file = database.prepare('SELECT kind, size, modified_ms, source_path, hash FROM files WHERE watch_id = ? AND path = ?').get(watchId, path)
  if (!file?.source_path) return undefined
  const sourcePath = String(file.source_path)
  const displayPath = mirrorPath(sourcePath)
  const entry: MirrorEntry =
    file.kind === 'directory'
      ? { kind: 'directory', path: displayPath }
      : { kind: 'file', path: displayPath, size: Number(file.size), modifiedMs: Number(file.modified_ms), hash: String(file.hash) }
  return { sourcePath, change: { kind: 'upsert', entry } }
}

export function catalogueChange(database: DatabaseSync, watchId: string, path: string, deleted = false, previous?: MirrorEntry) {
  const entry = catalogueEntry(database, watchId, path)
  if (!entry) return
  enqueueChange(database, entry.sourcePath, deleted ? { kind: 'delete', path: entry.change.entry.path } : entry.change, deleted ? entry.change.entry : previous)
}

export function catalogueAll(database: DatabaseSync) {
  for (const row of database.prepare('SELECT watch_id, path FROM files').all()) {
    catalogueChange(database, String(row.watch_id), String(row.path))
  }
}

export function retireSyncScope(database: DatabaseSync, watchId: string) {
  const scopes = database.prepare('SELECT path, kind FROM watch_locations WHERE id <> ?').all(watchId)
  for (const row of database.prepare('SELECT path_key, change FROM sync_pending').all()) {
    const change: Change = JSON.parse(String(row.change))
    if (scopes.some((scope) => coversPath({ path: String(scope.path), kind: scope.kind }, changePath(change)))) continue
    database.prepare('DELETE FROM sync_pending WHERE path_key = ?').run(row.path_key)
    database.prepare('UPDATE sync_operations SET abort_requested = 1 WHERE path_key = ?').run(row.path_key)
    database
      .prepare(`UPDATE sync_operations SET abort_requested = 1 WHERE operation_id IN
      (SELECT operation_id FROM sync_operation_members WHERE path_key = ?)`)
      .run(row.path_key)
  }
}
