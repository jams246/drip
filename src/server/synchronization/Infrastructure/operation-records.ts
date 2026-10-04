import { createHash } from 'node:crypto'
import { SyncError } from '../Domain/errors.js'
import { type Offer, type Operation, type Root, canonicalPath } from '../Domain/models.js'
import { assertPublicationNamespace, assertRevision } from '../Domain/root-policy.js'
import { parseOffer } from '../Domain/validation.js'
import type { SynchronizationRepository } from '../Application/ports.js'
import { transaction } from './database.js'
import { RootRecords } from './root-records.js'

const OFFER_LIFETIME_MS = 86_400_000

export class SqliteSynchronizationRepository extends RootRecords implements SynchronizationRepository {
  private readonly payloads = new Map<string, Offer>()

  operation(deviceId: string, rootId: string, operationId: string): Operation {
    this.root(deviceId, rootId)
    let offer = this.payloads.get(operationId)
    const columns = offer ? 'status,revision' : 'payload,status,revision'
    const row = this.database.prepare(`SELECT ${columns} FROM operations WHERE id=? AND root_id=?`).get(operationId, rootId)
    if (!row) throw new SyncError('not_found', 'Operation not found.')
    if (!offer) {
      offer = parseOffer(JSON.parse(String(row.payload)))
      if (row.status === 'offered' || row.status === 'publishing') this.payloads.set(operationId, offer)
    }
    if (row.status !== 'offered' && row.status !== 'publishing' && row.status !== 'committed' && row.status !== 'aborted')
      throw new SyncError('storage_failure', 'Stored operation status is invalid.')
    return { ...offer, rootId, deviceId, status: row.status, revision: Number(row.revision) }
  }

  offer(root: Root, offer: Offer): Operation {
    const payload = JSON.stringify(offer)
    const fingerprint = createHash('sha256').update(payload).digest('hex')
    const identity = this.database.prepare('SELECT root_id,fingerprint FROM operation_fingerprints WHERE operation_id=?').get(offer.operationId)
    if (identity && (identity.root_id !== root.rootId || identity.fingerprint !== fingerprint))
      throw new SyncError('conflict', 'Operation identity cannot change payload.')
    const previous = this.database.prepare('SELECT root_id,payload FROM operations WHERE id=?').get(offer.operationId)
    if (previous) {
      if (previous.root_id !== root.rootId || previous.payload !== payload) throw new SyncError('conflict', 'Operation identity cannot change payload.')
      return this.operation(root.deviceId, root.rootId, offer.operationId)
    }
    assertRevision(root.revision, offer.baseRevision)
    if (this.database.prepare("SELECT 1 FROM operations WHERE root_id=? AND status IN ('offered','publishing')").get(root.rootId)) {
      throw new SyncError('conflict', 'Root already has an active operation.')
    }
    transaction(this.database, () => {
      this.database.prepare('INSERT OR IGNORE INTO operation_fingerprints VALUES(?,?,?)').run(offer.operationId, root.rootId, fingerprint)
      this.database
        .prepare('INSERT INTO operations VALUES(?,?,?,?,?,?,?)')
        .run(offer.operationId, root.rootId, offer.baseRevision, payload, 'offered', root.revision, Date.now() + OFFER_LIFETIME_MS)
      if (offer.change.kind === 'upsert') this.pin(offer.operationId, offer.change.chunks)
    })
    this.payloads.set(offer.operationId, offer)
    return this.operation(root.deviceId, root.rootId, offer.operationId)
  }

  abort(operation: Operation) {
    transaction(this.database, () => {
      this.database.prepare("UPDATE operations SET status='aborted' WHERE id=? AND status='offered'").run(operation.operationId)
      this.database.prepare('DELETE FROM operation_pins WHERE operation_id=?').run(operation.operationId)
    })
    this.payloads.delete(operation.operationId)
  }

  missing(operation: Operation): string[] {
    return this.database
      .prepare(`SELECT p.hash FROM operation_pins p LEFT JOIN chunks c ON c.hash=p.hash
      WHERE p.operation_id=? AND (c.hash IS NULL OR c.length<>p.length OR c.deleting=1) ORDER BY p.hash`)
      .all(operation.operationId)
      .map((row) => String(row.hash))
  }

  prepare(operation: Operation) {
    const root = this.root(operation.deviceId, operation.rootId)
    assertRevision(root.revision, operation.baseRevision)
    this.assertNamespace(root, operation)
    transaction(this.database, () => {
      this.database.prepare("UPDATE operations SET status='publishing' WHERE id=? AND status='offered'").run(operation.operationId)
      this.database.prepare('INSERT OR IGNORE INTO publication_journal VALUES(?)').run(operation.operationId)
    })
  }

  finalize(operation: Operation) {
    transaction(this.database, () => {
      const changed = this.database
        .prepare('UPDATE roots SET revision=revision+1 WHERE id=? AND revision=? AND retired=0')
        .run(operation.rootId, operation.baseRevision)
      if (changed.changes !== 1) throw new SyncError('revision_conflict', 'Root revision changed during publication.')
      this.replaceHead(operation)
      this.database.prepare("UPDATE operations SET status='committed',revision=? WHERE id=?").run(operation.baseRevision + 1, operation.operationId)
      this.database.prepare('DELETE FROM operation_pins WHERE operation_id=?').run(operation.operationId)
      this.database.prepare('DELETE FROM publication_journal WHERE operation_id=?').run(operation.operationId)
      this.database.prepare("DELETE FROM operations WHERE root_id=? AND status='committed' AND id<>?").run(operation.rootId, operation.operationId)
    })
    this.payloads.delete(operation.operationId)
  }

  publishing(): Operation[] {
    return this.database
      .prepare(
        'SELECT o.id,o.root_id,r.device_id FROM publication_journal j JOIN operations o ON o.id=j.operation_id JOIN roots r ON r.id=o.root_id WHERE r.retired=0'
      )
      .all()
      .map((row) => this.operation(String(row.device_id), String(row.root_id), String(row.id)))
  }

  expire() {
    const rows = this.database
      .prepare(`SELECT o.id FROM operations o JOIN roots r ON r.id=o.root_id WHERE o.status='offered' AND o.expires_at<?
      AND r.retired=0 AND NOT EXISTS(SELECT 1 FROM device_fences f WHERE f.device_id=r.device_id)
      AND NOT EXISTS(SELECT 1 FROM publication_journal j WHERE j.operation_id=o.id)`)
      .all(Date.now())
    if (!rows.length) return
    transaction(this.database, () => {
      const remove = this.database.prepare("DELETE FROM operations WHERE id=? AND status='offered'")
      for (const row of rows) remove.run(row.id)
    })
    for (const row of rows) this.payloads.delete(String(row.id))
  }

  private pin(operationId: string, chunks: { hash: string; length: number }[]) {
    const insert = this.database.prepare('INSERT OR IGNORE INTO operation_pins VALUES(?,?,?)')
    for (const chunk of chunks) insert.run(operationId, chunk.hash, chunk.length)
  }

  private assertNamespace(root: Root, operation: Operation) {
    if (operation.change.kind === 'delete') return
    const path = canonicalPath(operation.change.path)
    const ancestors = path.split('/')
    ancestors.pop()
    let ancestorFile = false
    while (ancestors.length) {
      if (this.database.prepare('SELECT 1 FROM heads WHERE root_id=? AND path_key=?').get(root.rootId, ancestors.join('/'))) {
        ancestorFile = true
        break
      }
      ancestors.pop()
    }
    const prefix = path + '/'
    const descendants = Boolean(
      this.database.prepare('SELECT 1 FROM heads WHERE root_id=? AND path_key>=? AND path_key<?').get(root.rootId, prefix, path + '0')
    )
    const otherFile = root.kind === 'file' && Boolean(this.database.prepare('SELECT 1 FROM heads WHERE root_id=? AND path_key<>?').get(root.rootId, path))
    assertPublicationNamespace(root, operation.change, { ancestorFile, descendants, otherFile })
  }

  private replaceHead(operation: Operation) {
    const path = canonicalPath(operation.change.path)
    this.database.prepare('DELETE FROM heads WHERE root_id=? AND path_key=?').run(operation.rootId, path)
    if (operation.change.kind === 'delete') return
    const { path: displayPath, size, modifiedMs, chunks } = operation.change
    this.database.prepare('INSERT INTO heads VALUES(?,?,?)').run(operation.rootId, path, JSON.stringify({ path: displayPath, size, modifiedMs, chunks }))
    const insert = this.database.prepare('INSERT OR IGNORE INTO head_chunks VALUES(?,?,?)')
    for (const chunk of chunks) insert.run(operation.rootId, path, chunk.hash)
  }
}
