import type { DatabaseSync } from 'node:sqlite'
import type { Change, MirrorEntry } from '../../protocol/sync'
import { openDatabase, transaction } from '../storage/database'
import { catalogueAll, catalogueEntry, changePath, coversPath, enqueueChange, mirrorPath } from '../storage/sync-outbox'
import {
  acknowledgeOperation,
  currentOperation,
  disableMove,
  discardOperation,
  freezeOperation,
  operationMembers,
  readOperation,
  readRegions
} from './operations'
import type { FrozenOperation, OperationMember, SyncCredentials, SyncRoot } from './types'
const SYNC_WRITE_TIMEOUT_MS = 5000

export class SyncStore {
  readonly database: DatabaseSync
  constructor(readonly databasePath: string) {
    this.database = openDatabase(databasePath, true, SYNC_WRITE_TIMEOUT_MS)
  }

  credentials(): SyncCredentials | undefined {
    const row = this.database.prepare('SELECT * FROM sync_credentials WHERE id = 1').get()
    return row
      ? {
          url: String(row.url),
          deviceId: String(row.device_id),
          secret: String(row.secret),
          name: String(row.name),
          token: String(row.token),
          confirmed: Boolean(row.confirmed)
        }
      : undefined
  }

  saveCredentials(value: SyncCredentials) {
    const prior = this.credentials()
    transaction(this.database, () => {
      if (!prior || prior.url !== value.url || prior.deviceId !== value.deviceId) {
        this.database.exec('DELETE FROM frozen_regions; DELETE FROM sync_operation_members; DELETE FROM sync_operations; DELETE FROM sync_pending')
        this.database.prepare('UPDATE sync_state SET root_id = ?, registered = 0, revision = 0 WHERE id = 1').run(value.deviceId)
        catalogueAll(this.database)
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
    const credentials = this.credentials()
    if (!credentials) return []
    const state = this.database.prepare('SELECT * FROM sync_state WHERE id = 1').get()!
    return [{ rootId: credentials.deviceId, name: credentials.name, active: true, registered: Boolean(state.registered), revision: Number(state.revision) }]
  }

  isActive(rootId: string) {
    return this.credentials()?.deviceId === rootId
  }
  registered(rootId: string, revision: number) {
    transaction(this.database, () => {
      this.database.prepare('UPDATE sync_state SET registered = 1, revision = ?, root_id = ? WHERE id = 1').run(revision, rootId)
      this.database.prepare('UPDATE sync_pending SET root_id = ?').run(rootId)
      catalogueAll(this.database)
    })
  }
  revision(_rootId: string, revision: number) {
    this.database.prepare('UPDATE sync_state SET revision = ? WHERE id = 1').run(revision)
  }
  operation(_rootId?: string) {
    return readOperation(this.database)
  }
  freeze(rootId: string) {
    return freezeOperation(this.database, rootId, (member) => this.operationScopes(member).length > 0)
  }
  members(operation: FrozenOperation) {
    return operationMembers(this.database, operation)
  }
  regions(operation: FrozenOperation, after = -1, limit = 1000) {
    return readRegions(this.database, operation, after, limit)
  }
  current(operation: FrozenOperation) {
    return currentOperation(this.database, operation) && this.members(operation).every((member) => this.operationScopes(member).length > 0)
  }
  acknowledge(operation: FrozenOperation, revision: number) {
    acknowledgeOperation(this.database, operation, revision)
  }
  discard(operation: FrozenOperation, removePending = false) {
    discardOperation(this.database, operation, removePending)
  }
  disableMove(operation: FrozenOperation) {
    disableMove(this.database, operation)
  }
  requestAbort(operation: FrozenOperation) {
    this.database.prepare('UPDATE sync_operations SET abort_requested = 1 WHERE operation_id = ?').run(operation.operationId)
  }
  pendingCount() {
    return Number(
      this.database
        .prepare(`SELECT (SELECT COUNT(*) FROM sync_pending) +
      (SELECT COUNT(*) FROM sync_operations WHERE path_key NOT IN (SELECT path_key FROM sync_pending)) AS count`)
        .get()!.count
    )
  }

  readyScopes() {
    return this.database
      .prepare(`SELECT id, path, kind, coverage FROM watch_locations location WHERE scan_pending = 0
      AND NOT EXISTS (SELECT 1 FROM pending_changes WHERE watch_id = location.id)`)
      .all()
      .map((row) => ({ id: String(row.id), path: String(row.path), kind: row.kind, coverage: Boolean(row.coverage) }))
  }
  safeScopes() {
    return this.readyScopes().filter((scope) => scope.coverage)
  }
  operationScopes(member: OperationMember) {
    const path = changePath(member.change)
    return this.readyScopes()
      .map((scope) => ({ ...scope, proofPath: member.change.kind === 'delete' ? this.obstructionPath(scope, path) : '' }))
      .filter(
        (scope) => Boolean(scope.proofPath) || (coversPath(scope, path) && (member.change.kind !== 'delete' || Boolean(member.sourcePath) || scope.coverage))
      )
  }
  private obstructionPath(scope: { id: string; path: string; kind: unknown; coverage: boolean }, path: string) {
    if (scope.kind === 'file' && !scope.coverage) return ''
    let parent = mirrorPath(path).toLowerCase()
    while (parent.lastIndexOf('/') > 2) {
      parent = parent.slice(0, parent.lastIndexOf('/'))
      if (!coversPath(scope, parent)) continue
      const row = this.database.prepare("SELECT source_path FROM files WHERE watch_id=? AND path=? AND kind='file' AND hash<>''").get(scope.id, parent)
      if (row) return String(row.source_path)
    }
    return ''
  }
  watchIds(path: string) {
    return this.database
      .prepare('SELECT id, path, kind FROM watch_locations')
      .all()
      .filter((row) => coversPath({ path: String(row.path), kind: row.kind }, path))
      .map((row) => String(row.id))
  }

  reconcile(_root: SyncRoot, heads: MirrorEntry[]) {
    // oxlint-disable-next-line eslint/max-statements -- Reconcile ready entries while keeping complete and file-obstructed absence proofs separate.
    transaction(this.database, () => {
      const scopes = this.readyScopes()
      const safe = scopes.filter((scope) => scope.coverage)
      const owners = new Set(scopes.map((scope) => scope.id))
      const safeOwners = new Set(safe.map((scope) => scope.id))
      const safelyPresent = new Set<string>()
      const local = new Map<string, { sourcePath: string; change: Change & { kind: 'upsert' } }>()
      for (const row of this.database.prepare('SELECT watch_id, path FROM files').all()) {
        if (!owners.has(String(row.watch_id))) continue
        const entry = catalogueEntry(this.database, String(row.watch_id), String(row.path))
        if (entry && scopes.some((scope) => coversPath(scope, entry.change.entry.path))) {
          if (scopes.some((scope) => this.obstructionPath(scope, entry.change.entry.path))) continue
          const pathKey = entry.change.entry.path.toLowerCase()
          local.set(pathKey, entry)
          if (safeOwners.has(String(row.watch_id))) safelyPresent.add(pathKey)
        }
      }
      for (const head of heads) {
        const obstructed = scopes.some((scope) => this.obstructionPath(scope, head.path))
        if (!obstructed && !scopes.some((scope) => coversPath(scope, head.path))) continue
        const entry = local.get(head.path.toLowerCase())
        if (obstructed || (safe.some((scope) => coversPath(scope, head.path)) && !safelyPresent.has(head.path.toLowerCase()))) {
          enqueueChange(this.database, '', { kind: 'delete', path: head.path }, head)
        } else if (entry && JSON.stringify(entry.change.entry) !== JSON.stringify(head)) enqueueChange(this.database, entry.sourcePath, entry.change, head)
        local.delete(head.path.toLowerCase())
      }
      for (const entry of local.values()) enqueueChange(this.database, entry.sourcePath, entry.change)
    })
  }

  close() {
    this.database.close()
  }
}
