import type { DatabaseSync } from 'node:sqlite'
import { recordDiagnostic } from '../diagnostics'
import { openDatabase, transaction } from '../storage/database'
import { catalogueChange, catalogueEntry, enqueueChange } from '../storage/sync-outbox'
import { randomId } from './identity'
import type { FrozenOperation, SyncCredentials, SyncRoot } from './types'
import type { Change, FileHead } from '../../protocol/sync'
const SYNC_WRITE_TIMEOUT_MS = 5000

function rootFromRow(row: Record<string, unknown>): SyncRoot {
  return {
    watchId: String(row.watch_id),
    rootId: String(row.root_id),
    name: String(row.name),
    kind: row.kind === 'file' ? 'file' : 'folder',
    active: Boolean(row.active),
    registered: Boolean(row.registered),
    revision: Number(row.revision)
  }
}

function operationFromRow(row: Record<string, unknown>): FrozenOperation {
  return {
    rootId: String(row.root_id),
    operationId: String(row.operation_id),
    baseRevision: Number(row.base_revision),
    pathKey: String(row.path_key),
    generation: Number(row.generation),
    sourcePath: String(row.source_path),
    change: JSON.parse(String(row.change)),
    abortRequested: Boolean(row.abort_requested)
  }
}

export class SyncStore {
  readonly database: DatabaseSync

  constructor(databasePath: string) {
    this.database = openDatabase(databasePath, true, SYNC_WRITE_TIMEOUT_MS)
  }

  credentials(): SyncCredentials | undefined {
    const row = this.database.prepare('SELECT url, device_id, secret, name, token, confirmed FROM sync_credentials WHERE id = 1').get()
    if (!row) return undefined
    return {
      url: String(row.url),
      deviceId: String(row.device_id),
      secret: String(row.secret),
      name: String(row.name),
      token: String(row.token),
      confirmed: Boolean(row.confirmed)
    }
  }

  saveCredentials(value: SyncCredentials) {
    const prior = this.database.prepare('SELECT url, device_id FROM sync_credentials WHERE id = 1').get()
    transaction(this.database, () => {
      if (prior && (prior.url !== value.url || prior.device_id !== value.deviceId)) {
        this.database.exec('DELETE FROM sync_operations; DELETE FROM sync_pending; UPDATE sync_roots SET registered = 0, revision = 0')
        const roots = this.database.prepare('SELECT root_id FROM sync_roots').all()
        for (const root of roots) this.database.prepare('UPDATE sync_roots SET root_id = ? WHERE root_id = ?').run(randomId(), root.root_id)
      }
      this.database
        .prepare(`INSERT INTO sync_credentials (id, url, device_id, secret, name, token, confirmed) VALUES (1, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET url = excluded.url, device_id = excluded.device_id, secret = excluded.secret,
        name = excluded.name, token = excluded.token, confirmed = excluded.confirmed`)
        .run(value.url, value.deviceId, value.secret, value.name, value.token, value.confirmed ? 1 : 0)
    })
  }

  confirm() {
    this.database.prepare("UPDATE sync_credentials SET confirmed = 1, token = '' WHERE id = 1").run()
  }

  roots(): SyncRoot[] {
    return this.database.prepare('SELECT watch_id, root_id, name, kind, active, registered, revision FROM sync_roots ORDER BY watch_id').all().map(rootFromRow)
  }

  isActive(rootId: string): boolean {
    return this.database.prepare('SELECT 1 FROM sync_roots WHERE root_id = ? AND active = 1').get(rootId) !== undefined
  }

  registered(rootId: string, revision: number) {
    transaction(this.database, () => {
      this.database.prepare('UPDATE sync_roots SET registered = 1, revision = ? WHERE root_id = ?').run(revision, rootId)
      const root = this.database.prepare('SELECT watch_id FROM sync_roots WHERE root_id = ?').get(rootId)!
      const files = this.database.prepare('SELECT path FROM files WHERE watch_id = ?').all(root.watch_id)
      for (const file of files) catalogueChange(this.database, String(root.watch_id), String(file.path))
    })
  }

  revision(rootId: string, revision: number) {
    this.database.prepare('UPDATE sync_roots SET revision = ? WHERE root_id = ?').run(revision, rootId)
  }

  operation(rootId: string): FrozenOperation | undefined {
    const row = this.database.prepare('SELECT * FROM sync_operations WHERE root_id = ?').get(rootId)
    return row ? operationFromRow(row) : undefined
  }

  freeze(rootId: string): FrozenOperation | undefined {
    transaction(this.database, () => {
      if (this.database.prepare('SELECT 1 FROM sync_operations WHERE root_id = ?').get(rootId)) return
      const pending = this.database
        .prepare(`SELECT path_key, generation, source_path, change FROM sync_pending WHERE root_id = ?
        ORDER BY CASE WHEN instr(change, '"kind":"delete"') > 0 THEN 0 ELSE 1 END,
        CASE WHEN instr(change, '"kind":"delete"') > 0 THEN length(path_key) - length(replace(path_key, '/', '')) ELSE 0 END DESC,
        generation LIMIT 1`)
        .get(rootId)
      const root = this.database.prepare('SELECT revision, active FROM sync_roots WHERE root_id = ?').get(rootId)
      if (!pending || !root?.active) return
      recordDiagnostic('sync.operation.freeze.start', `root=${rootId} revision=${String(root.revision)}`)
      this.database
        .prepare(`INSERT INTO sync_operations (root_id, operation_id, base_revision, path_key, generation, source_path, change)
        VALUES (?, ?, ?, ?, ?, ?, ?)`)
        .run(rootId, randomId(), root.revision, pending.path_key, pending.generation, pending.source_path, pending.change)
    })
    const operation = this.operation(rootId)
    if (operation) recordDiagnostic('sync.operation.freeze.result', `root=${rootId} operation=${operation.operationId}`)
    return operation
  }

  acknowledge(operation: FrozenOperation, revision: number) {
    transaction(this.database, () => {
      this.database.prepare('UPDATE sync_roots SET revision = ? WHERE root_id = ?').run(revision, operation.rootId)
      this.database
        .prepare('DELETE FROM sync_pending WHERE root_id = ? AND path_key = ? AND generation = ?')
        .run(operation.rootId, operation.pathKey, operation.generation)
      this.database.prepare('DELETE FROM sync_operations WHERE root_id = ? AND operation_id = ?').run(operation.rootId, operation.operationId)
    })
  }

  discard(operation: FrozenOperation, removePending = false) {
    transaction(this.database, () => {
      this.database.prepare('DELETE FROM sync_operations WHERE operation_id = ?').run(operation.operationId)
      if (removePending)
        this.database
          .prepare('DELETE FROM sync_pending WHERE root_id = ? AND path_key = ? AND generation = ?')
          .run(operation.rootId, operation.pathKey, operation.generation)
    })
  }

  requestAbort(operation: FrozenOperation) {
    this.database.prepare('UPDATE sync_operations SET abort_requested = 1 WHERE operation_id = ?').run(operation.operationId)
  }

  pendingCount(): number {
    return Number(
      this.database
        .prepare(`SELECT (SELECT COUNT(*) FROM sync_pending) + (SELECT COUNT(*) FROM sync_operations operation
        WHERE NOT EXISTS (SELECT 1 FROM sync_pending pending
        WHERE pending.root_id = operation.root_id AND pending.path_key = operation.path_key)) AS count`)
        .get()!.count
    )
  }

  safeCoverage(root: SyncRoot): boolean {
    const current = this.database.prepare('SELECT active, coverage FROM sync_roots WHERE root_id = ?').get(root.rootId)
    const location = this.database.prepare('SELECT scan_pending FROM watch_locations WHERE id = ?').get(root.watchId)
    const pending = this.database.prepare('SELECT 1 FROM pending_changes WHERE watch_id = ? LIMIT 1').get(root.watchId)
    return Boolean(current?.active && current.coverage && location && !location.scan_pending && !pending)
  }

  reconcile(root: SyncRoot, heads: FileHead[]) {
    transaction(this.database, () => {
      const known = new Set<string>()
      for (const head of heads) {
        known.add(head.path.toLowerCase())
        const localPath = root.kind === 'file' ? root.watchId : root.watchId.replace(/\/$/, '') + '/' + head.path.toLowerCase()
        const local = catalogueEntry(this.database, root.watchId, localPath)
        if (!local) {
          enqueueChange(this.database, root.rootId, '', { kind: 'delete', path: head.path })
          continue
        }
        if (
          local.change.kind === 'upsert' &&
          (local.change.path !== head.path ||
            local.change.size !== head.size ||
            local.change.modifiedMs !== head.modifiedMs ||
            JSON.stringify(local.change.chunks) !== JSON.stringify(head.chunks))
        ) {
          enqueueChange(this.database, root.rootId, local.sourcePath, local.change)
        }
      }
      const files = this.database.prepare('SELECT path FROM files WHERE watch_id = ?').all(root.watchId)
      for (const file of files) {
        const entry = catalogueEntry(this.database, root.watchId, String(file.path))
        if (entry && !known.has(entry.change.path.toLowerCase())) enqueueChange(this.database, root.rootId, entry.sourcePath, entry.change)
      }
    })
  }

  enqueue(rootId: string, sourcePath: string, change: Change) {
    transaction(this.database, () => enqueueChange(this.database, rootId, sourcePath, change))
  }

  close() {
    this.database.close()
  }
}
