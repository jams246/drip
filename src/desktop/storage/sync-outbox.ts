import type { DatabaseSync } from 'node:sqlite'
import { basename, relative } from 'node:path'
import { Buffer } from 'node:buffer'
import { CHUNK_PROFILE, type Change, type Chunk } from '../../protocol/sync'
import { randomId } from '../sync/identity'
import type { ScanLocation } from '../scan/types'

export function bindSyncRoot(database: DatabaseSync, item: ScanLocation) {
  const prior = database.prepare('SELECT root_id, kind FROM sync_roots WHERE watch_id = ?').get(item.id)
  if (prior && prior.kind !== item.kind) {
    database.prepare('UPDATE sync_roots SET watch_id = ?, active = 0, coverage = 0 WHERE root_id = ?').run(item.id + '#' + prior.root_id, prior.root_id)
    database.prepare('DELETE FROM sync_pending WHERE root_id = ?').run(prior.root_id)
    database.prepare('UPDATE sync_operations SET abort_requested = 1 WHERE root_id = ?').run(prior.root_id)
  }
  database
    .prepare(`INSERT INTO sync_roots (watch_id, root_id, path, name, kind) VALUES (?, ?, ?, ?, ?)
    ON CONFLICT(watch_id) DO UPDATE SET active = 1, path = excluded.path, coverage = 0`)
    .run(item.id, randomId(), item.path, item.name, item.kind)
}

export function enqueueChange(database: DatabaseSync, rootId: string, sourcePath: string, change: Change) {
  database.prepare('UPDATE sync_state SET generation = generation + 1 WHERE id = 1').run()
  const generation = Number(database.prepare('SELECT generation FROM sync_state WHERE id = 1').get()!.generation)
  database
    .prepare(`INSERT INTO sync_pending (root_id, path_key, generation, source_path, change) VALUES (?, ?, ?, ?, ?)
    ON CONFLICT(root_id, path_key) DO UPDATE SET generation = excluded.generation, source_path = excluded.source_path, change = excluded.change`)
    .run(rootId, change.path.toLowerCase(), generation, sourcePath, JSON.stringify(change))
}

export function catalogueEntry(database: DatabaseSync, watchId: string, path: string): { rootId: string; sourcePath: string; change: Change } | undefined {
  const root = database.prepare('SELECT root_id, path, kind FROM sync_roots WHERE watch_id = ? AND active = 1').get(watchId)
  if (!root) return undefined
  const file = database.prepare('SELECT id, size, modified_ms, source_path FROM files WHERE watch_id = ? AND path = ?').get(watchId, path)
  if (!file) return undefined
  const sourcePath = String(file.source_path)
  if (!sourcePath) return undefined
  const displayPath = root.kind === 'file' ? basename(sourcePath) : relative(String(root.path), sourcePath).split('\\').join('/')
  if (!displayPath || displayPath.startsWith('../') || displayPath === '..') throw new Error('File is outside its synchronization root.')
  const chunks: Chunk[] = database
    .prepare('SELECT offset, length, hash FROM chunks WHERE file_id = ? ORDER BY offset')
    .all(file.id)
    .map((row) => ({
      offset: Number(row.offset),
      length: Number(row.length),
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- SQLite hash BLOB columns return byte arrays.
      hash: Buffer.from(row.hash as Uint8Array).toString('hex')
    }))
  const change: Change = { kind: 'upsert', path: displayPath, profile: CHUNK_PROFILE, size: Number(file.size), modifiedMs: Number(file.modified_ms), chunks }
  return { rootId: String(root.root_id), sourcePath, change }
}

export function catalogueChange(database: DatabaseSync, watchId: string, path: string, deleted = false) {
  const entry = catalogueEntry(database, watchId, path)
  if (!entry) return
  enqueueChange(database, entry.rootId, entry.sourcePath, deleted ? { kind: 'delete', path: entry.change.path } : entry.change)
}
