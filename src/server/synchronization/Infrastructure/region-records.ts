import { SyncError } from '../Domain/errors.js'
import type { Operation, PlannedRegion, RegionFingerprint, RegionPage } from '../Domain/models.js'
import { transaction } from './database.js'
import { RootRecords } from './root-records.js'

const OFFER_LIFETIME_MS = 86_400_000

export class RegionRecords extends RootRecords {
  addRegions(operation: Operation, regions: RegionFingerprint[]) {
    const change = operation.change
    if (operation.status !== 'offered' || change.kind !== 'upsert' || change.entry.kind !== 'file' || operation.metadataOnly)
      throw new SyncError('conflict', 'Operation does not accept comparison regions.')
    const fileSize = change.entry.size
    transaction(this.database, () => {
      let next = Number(
        this.database.prepare('SELECT COALESCE(MAX(offset+length),0) AS next FROM operation_regions WHERE operation_id=?').get(operation.operationId)!.next
      )
      let previousEnd = regions[0].offset
      for (const region of regions) {
        if (region.offset !== previousEnd || region.offset + region.length > fileSize)
          throw new SyncError('invalid_request', 'Regions must be contiguous and bounded by the file size.')
        previousEnd += region.length
        const existing = this.region(operation.operationId, region.offset)
        if (existing) {
          if (existing.length !== region.length || existing.hash !== region.hash) throw new SyncError('conflict', 'Comparison regions are immutable.')
          continue
        }
        if (operation.planComplete || region.offset !== next) throw new SyncError('conflict', 'Append comparison regions in order before sealing the plan.')
        const basis =
          operation.basis &&
          this.database
            .prepare('SELECT offset FROM head_regions WHERE root_id=? AND path_key=? AND hash=? AND length=? ORDER BY offset LIMIT 1')
            .get(operation.rootId, operation.basis.path.toLowerCase(), region.hash, region.length)
        this.database
          .prepare('INSERT INTO operation_regions(operation_id,offset,length,hash,basis_offset) VALUES(?,?,?,?,?)')
          .run(operation.operationId, region.offset, region.length, region.hash, basis ? Number(basis.offset) : null)
        next += region.length
      }
      this.touch(operation.operationId)
    })
  }

  seal(operation: Operation) {
    if (operation.status !== 'offered') throw new SyncError('conflict', 'Operation is not accepting a region plan.')
    if (operation.planComplete) return
    if (operation.change.kind !== 'upsert' || operation.change.entry.kind !== 'file') throw new SyncError('invalid_request', 'Only files have region plans.')
    const length = Number(
      this.database.prepare('SELECT COALESCE(MAX(offset+length),0) AS length FROM operation_regions WHERE operation_id=?').get(operation.operationId)!.length
    )
    if (length !== operation.change.entry.size) throw new SyncError('regions_missing', 'Complete the file comparison plan before sealing it.')
    if (!this.database.prepare('SELECT 1 FROM operation_regions WHERE operation_id=? AND basis_offset IS NOT NULL LIMIT 1').get(operation.operationId))
      this.database.prepare('UPDATE operations SET basis_manifest=NULL WHERE id=?').run(operation.operationId)
    this.database.prepare("UPDATE operations SET plan_complete=1 WHERE id=? AND status='offered'").run(operation.operationId)
    this.touch(operation.operationId)
  }

  ready(operationId: string) {
    this.database.prepare("UPDATE operations SET stage_ready=1 WHERE id=? AND status='offered'").run(operationId)
    this.touch(operationId)
  }

  uploaded(operationId: string, offset: number) {
    this.database.prepare('UPDATE operation_regions SET uploaded=1 WHERE operation_id=? AND offset=?').run(operationId, offset)
    this.touch(operationId)
  }

  region(operationId: string, offset: number): PlannedRegion | undefined {
    const row = this.database.prepare('SELECT * FROM operation_regions WHERE operation_id=? AND offset=?').get(operationId, offset)
    return row ? plannedRegion(row) : undefined
  }

  regions(operationId: string, after = -1, limit = 1000): PlannedRegion[] {
    return this.database
      .prepare('SELECT * FROM operation_regions WHERE operation_id=? AND offset>? ORDER BY offset LIMIT ?')
      .all(operationId, after, limit)
      .map(plannedRegion)
  }

  required(operationId: string, after: number, limit: number): RegionPage {
    const rows = this.database
      .prepare('SELECT offset FROM operation_regions WHERE operation_id=? AND offset>? AND basis_offset IS NULL AND uploaded=0 ORDER BY offset LIMIT ?')
      .all(operationId, after, limit + 1)
    return { required: rows.slice(0, limit).map((row) => Number(row.offset)), next: rows.length > limit ? Number(rows[limit - 1].offset) : null }
  }

  hasMissing(operationId: string): boolean {
    return Boolean(
      this.database.prepare('SELECT 1 FROM operation_regions WHERE operation_id=? AND basis_offset IS NULL AND uploaded=0 LIMIT 1').get(operationId)
    )
  }

  protected touch(operationId: string) {
    this.database.prepare("UPDATE operations SET expires_at=? WHERE id=? AND status='offered'").run(Date.now() + OFFER_LIFETIME_MS, operationId)
  }
}

function plannedRegion(row: Record<string, unknown>): PlannedRegion {
  return {
    offset: Number(row.offset),
    length: Number(row.length),
    hash: String(row.hash),
    basisOffset: row.basis_offset === null ? null : Number(row.basis_offset),
    uploaded: Boolean(row.uploaded)
  }
}
