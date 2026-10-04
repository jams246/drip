import type { DatabaseSync, StatementSync } from 'node:sqlite'
import { basename } from 'node:path'
import { initialScan } from '../scan/state'
import type { ScanLocation, ScanSnapshot } from '../scan/types'
import { openDatabase, transaction } from './database'
import { MonitoringStore } from './monitoring'
import { bindSyncRoot } from './sync-outbox'

export class WatchStore {
  private readonly database: DatabaseSync
  readonly monitoring: MonitoringStore
  private readonly selectLocations: StatementSync
  private readonly insertLocation: StatementSync
  private readonly updateScan: StatementSync
  private readonly markPending: StatementSync
  private readonly deleteLocation: StatementSync

  constructor(databasePath: string) {
    this.database = openDatabase(databasePath)
    try {
      this.monitoring = new MonitoringStore(this.database)
      this.selectLocations = this.database.prepare('SELECT id, path, kind, last_scan, scan_pending FROM watch_locations ORDER BY rowid')
      this.insertLocation = this.database.prepare(`INSERT INTO watch_locations (id, path, kind, scan_pending) VALUES (?, ?, ?, 1)
        ON CONFLICT(id) DO UPDATE SET path = excluded.path, kind = excluded.kind, scan_pending = 1`)
      this.updateScan = this.database.prepare('UPDATE watch_locations SET last_scan = ?, scan_pending = 0 WHERE id = ?')
      this.markPending = this.database.prepare('UPDATE watch_locations SET scan_pending = 1 WHERE id = ?')
      this.deleteLocation = this.database.prepare('DELETE FROM watch_locations WHERE id = ?')
    } catch (error) {
      this.database.close()
      throw error
    }
  }

  hydrate(): { locations: ScanLocation[]; scans: ScanSnapshot[] } {
    const locations: ScanLocation[] = []
    const scans: ScanSnapshot[] = []
    const records = this.selectLocations.all()
    for (const record of records) {
      const kind = record.kind
      if (kind !== 'file' && kind !== 'folder') throw new Error('Invalid saved watch location kind.')
      const path = String(record.path)
      const item: ScanLocation = {
        id: String(record.id),
        name: basename(path) || path,
        path,
        kind
      }
      locations.push(item)
      let scan: ScanSnapshot = record.last_scan
        ? { ...JSON.parse(String(record.last_scan)), id: item.id, path: item.path, kind: item.kind }
        : initialScan(item, 'queued')
      if (record.scan_pending) {
        scan = { ...scan, state: 'queued', currentPath: '', error: '' }
      }
      scans.push(scan)
    }
    return { locations, scans }
  }

  saveLocation(item: ScanLocation) {
    transaction(this.database, () => {
      this.insertLocation.run(item.id, item.path, item.kind)
      bindSyncRoot(this.database, item)
    })
  }

  finishScan(scan: ScanSnapshot) {
    const snapshot = {
      state: scan.state,
      bytes: scan.bytes,
      chunks: scan.chunks,
      files: scan.files,
      skipped: scan.skipped,
      errors: scan.errors,
      elapsedMs: scan.elapsedMs,
      currentPath: scan.currentPath,
      currentBytes: scan.currentBytes,
      currentSize: scan.currentSize,
      error: scan.error
    }
    transaction(this.database, () => {
      this.updateScan.run(JSON.stringify(snapshot), scan.id)
    })
  }

  startScan(id: string) {
    this.markPending.run(id)
  }

  removeLocation(id: string) {
    transaction(this.database, () => {
      const root = this.database.prepare('SELECT root_id FROM sync_roots WHERE watch_id = ?').get(id)
      this.database.prepare('UPDATE sync_roots SET active = 0, coverage = 0 WHERE watch_id = ?').run(id)
      if (root) {
        this.database.prepare('DELETE FROM sync_pending WHERE root_id = ?').run(root.root_id)
        this.database.prepare('UPDATE sync_operations SET abort_requested = 1 WHERE root_id = ?').run(root.root_id)
      }
      this.deleteLocation.run(id)
    })
  }

  close() {
    this.database.close()
  }
}
