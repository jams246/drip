import { createHash } from 'node:crypto'
import { SyncError } from '../Domain/errors.js'
import { type MirrorEntry, type Offer, type Operation, type Root, canonicalPath } from '../Domain/models.js'
import { assertRevision } from '../Domain/root-policy.js'
import { parseHead, parseOffer } from '../Domain/validation.js'
import { transaction } from './database.js'
import { replaceHeads } from './head-publication.js'
import { RegionRecords } from './region-records.js'

const OFFER_LIFETIME_MS = 86_400_000

export class SqliteSynchronizationRepository extends RegionRecords {
  operation(deviceId: string, rootId: string, operationId: string): Operation {
    this.root(deviceId, rootId)
    const row = this.database.prepare('SELECT * FROM operations WHERE id=? AND root_id=?').get(operationId, rootId)
    if (!row) throw new SyncError('not_found', 'Operation not found.')
    const offer = parseOffer(JSON.parse(String(row.payload)))
    if (row.status !== 'offered' && row.status !== 'publishing' && row.status !== 'committed' && row.status !== 'aborted')
      throw new SyncError('storage_failure', 'Stored operation status is invalid.')
    const basis = row.basis_manifest ? parseHead(JSON.parse(String(row.basis_manifest))) : undefined
    return {
      ...offer,
      rootId,
      deviceId,
      status: row.status,
      revision: Number(row.revision),
      planComplete: Boolean(row.plan_complete),
      stageReady: Boolean(row.stage_ready),
      metadataOnly: Boolean(row.metadata_only),
      basis: basis?.kind === 'file' ? basis : undefined
    }
  }

  offer(root: Root, offer: Offer): Operation {
    const payload = JSON.stringify(offer)
    const fingerprint = createHash('sha256').update(payload).digest('hex')
    const previous = this.database.prepare('SELECT root_id,fingerprint FROM operation_fingerprints WHERE operation_id=?').get(offer.operationId)
    if (previous) {
      if (previous.root_id !== root.rootId || previous.fingerprint !== fingerprint)
        throw new SyncError('conflict', 'Operation identity already has a different request.')
      return this.operation(root.deviceId, root.rootId, offer.operationId)
    }
    assertRevision(root.revision, offer.baseRevision)
    if (this.database.prepare("SELECT 1 FROM operations WHERE root_id=? AND status IN ('offered','publishing')").get(root.rootId))
      throw new SyncError('conflict', 'Another operation is active for this device.')
    this.assertNamespace(root, offer)
    const entry = offer.change.kind === 'upsert' ? offer.change.entry : undefined
    const previousHead = entry && this.head(root.rootId, entry.path)
    const basis = previousHead?.kind === 'file' ? previousHead : undefined
    const metadataOnly = entry?.kind === 'file' && basis?.hash === entry.hash && basis.size === entry.size
    const complete = !entry || entry.kind !== 'file' || metadataOnly
    transaction(this.database, () => {
      this.database.prepare('INSERT INTO operation_fingerprints VALUES(?,?,?)').run(offer.operationId, root.rootId, fingerprint)
      this.database
        .prepare(`INSERT INTO operations(id,root_id,base_revision,payload,status,revision,expires_at,plan_complete,stage_ready,metadata_only,basis_manifest)
        VALUES(?,?,?,?,'offered',?,?,?,?,?,?)`)
        .run(
          offer.operationId,
          root.rootId,
          offer.baseRevision,
          payload,
          root.revision,
          Date.now() + OFFER_LIFETIME_MS,
          complete ? 1 : 0,
          complete ? 1 : 0,
          metadataOnly ? 1 : 0,
          basis ? JSON.stringify(basis) : null
        )
      if (offer.change.kind === 'move')
        offer.change.moves.forEach((_move, index) =>
          this.database.prepare('INSERT INTO operation_moves(operation_id,position) VALUES(?,?)').run(offer.operationId, index)
        )
    })
    return this.operation(root.deviceId, root.rootId, offer.operationId)
  }

  abort(operation: Operation) {
    transaction(this.database, () => {
      this.database.prepare("UPDATE operations SET status='aborted' WHERE id=? AND status='offered'").run(operation.operationId)
      this.database.prepare('DELETE FROM operation_regions WHERE operation_id=?').run(operation.operationId)
      this.database.prepare('DELETE FROM operation_moves WHERE operation_id=?').run(operation.operationId)
    })
  }

  prepare(operation: Operation) {
    assertRevision(this.root(operation.deviceId, operation.rootId).revision, operation.baseRevision)
    this.assertNamespace(this.root(operation.deviceId, operation.rootId), operation)
    if (!operation.planComplete || !operation.stageReady || this.hasMissing(operation.operationId))
      throw new SyncError('regions_missing', 'Complete the comparison plan and uploads before committing.')
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
      if (changed.changes !== 1) throw new SyncError('revision_conflict', 'Device revision changed during publication.')
      replaceHeads(this.database, operation)
      this.database.prepare("UPDATE operations SET status='committed',revision=? WHERE id=?").run(operation.baseRevision + 1, operation.operationId)
      this.database.prepare('DELETE FROM publication_journal WHERE operation_id=?').run(operation.operationId)
      this.database.prepare('DELETE FROM operation_regions WHERE operation_id=?').run(operation.operationId)
      this.database.prepare('DELETE FROM operation_moves WHERE operation_id=?').run(operation.operationId)
    })
  }

  publishing(): Operation[] {
    return this.database
      .prepare(
        'SELECT o.id,o.root_id,r.device_id FROM publication_journal j JOIN operations o ON o.id=j.operation_id JOIN roots r ON r.id=o.root_id WHERE r.retired=0'
      )
      .all()
      .map((row) => this.operation(String(row.device_id), String(row.root_id), String(row.id)))
  }

  offered(expiredOnly = false): Operation[] {
    const condition = expiredOnly ? 'AND o.expires_at<?' : ''
    return this.database
      .prepare(`SELECT o.id,o.root_id,r.device_id FROM operations o JOIN roots r ON r.id=o.root_id WHERE o.status='offered' AND r.retired=0 ${condition}`)
      .all(...(expiredOnly ? [Date.now()] : []))
      .map((row) => this.operation(String(row.device_id), String(row.root_id), String(row.id)))
  }

  private assertNamespace(root: Root, offer: Offer) {
    const change = offer.change
    if (change.kind === 'delete') {
      this.assertDeletion(root, change.path)
      return
    }
    const movingSources = new Set(change.kind === 'move' ? change.moves.map((move) => canonicalPath(move.from)) : [])
    for (const entry of change.kind === 'move' ? change.moves.map((move) => move.entry) : [change.entry]) {
      this.assertAncestors(root, entry.path, movingSources)
      const canReplaceDirectory = entry.kind === 'file' && change.kind === 'move' && this.canReserveDescendants(root, entry.path, movingSources)
      this.assertReplacement(root, entry, canReplaceDirectory)
    }
    if (change.kind !== 'move') return
    for (const move of change.moves) {
      const source = this.head(root.rootId, move.from)
      if (source?.kind !== 'file' || source.hash !== move.entry.hash || source.size !== move.entry.size)
        throw new SyncError('conflict', 'Move source does not match the desired file.')
    }
  }

  private assertAncestors(root: Root, path: string, movingSources: Set<string>) {
    const parts = canonicalPath(path).split('/').slice(0, -1)
    while (parts.length) {
      const ancestor = parts.join('/')
      if (this.head(root.rootId, ancestor)?.kind === 'file' && !movingSources.has(ancestor))
        throw new SyncError('conflict', 'Move or delete the ancestor file before creating descendants.')
      parts.pop()
    }
  }

  private assertReplacement(root: Root, entry: MirrorEntry, canReplaceDirectory: boolean) {
    const prefix = canonicalPath(entry.path) + '/'
    if (
      entry.kind === 'file' &&
      !canReplaceDirectory &&
      this.database.prepare('SELECT 1 FROM heads WHERE root_id=? AND substr(path_key,1,?)=? LIMIT 1').get(root.rootId, prefix.length, prefix)
    )
      throw new SyncError('conflict', 'Move or delete descendants before replacing their directory.')
    const current = this.head(root.rootId, entry.path)
    if (current && current.kind !== entry.kind && !canReplaceDirectory) throw new SyncError('conflict', 'Delete the previous entry type before replacing it.')
  }

  private canReserveDescendants(root: Root, path: string, movingSources: Set<string>) {
    const prefix = canonicalPath(path) + '/'
    return this.database
      .prepare('SELECT path_key,manifest FROM heads WHERE root_id=? AND substr(path_key,1,?)=?')
      .all(root.rootId, prefix.length, prefix)
      .every((row) => parseHead(JSON.parse(String(row.manifest))).kind === 'directory' || movingSources.has(String(row.path_key)))
  }

  private assertDeletion(root: Root, path: string) {
    const key = canonicalPath(path)
    const prefix = key.endsWith('/') ? key : key + '/'
    if (
      this.database
        .prepare('SELECT 1 FROM heads WHERE root_id=? AND path_key<>? AND substr(path_key,1,?)=? LIMIT 1')
        .get(root.rootId, key, prefix.length, prefix)
    )
      throw new SyncError('conflict', 'Move or delete descendants before deleting their directory.')
  }
}
