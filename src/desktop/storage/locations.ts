import type { DatabaseSync } from 'node:sqlite'
import { basename } from 'node:path'
import { initialScan } from '../scan/state'
import type { ScanLocation, ScanSnapshot } from '../scan/types'
import { openDatabase, transaction } from './database'

export class WatchStore {
  private readonly database: DatabaseSync

  constructor(databasePath: string) {
    this.database = openDatabase(databasePath)
  }

  hydrate(): { locations: ScanLocation[]; scans: ScanSnapshot[] } {
    const locations: ScanLocation[] = []
    const scans: ScanSnapshot[] = []
    const records = this.database.prepare('SELECT id, path, kind, last_scan, scan_pending FROM watch_locations ORDER BY rowid').all()
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
        scan = { ...scan, state: 'error', currentPath: '', error: 'Scan interrupted. Add this location again to rescan.' }
      }
      scans.push(scan)
    }
    return { locations, scans }
  }

  saveLocation(item: ScanLocation) {
    transaction(this.database, () => {
      this.database
        .prepare(`INSERT INTO watch_locations (id, path, kind, scan_pending) VALUES (?, ?, ?, 1)
          ON CONFLICT(id) DO UPDATE SET path = excluded.path, kind = excluded.kind, scan_pending = 1`)
        .run(item.id, item.path, item.kind)
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
      this.database.prepare('UPDATE watch_locations SET last_scan = ?, scan_pending = 0 WHERE id = ?').run(JSON.stringify(snapshot), scan.id)
    })
  }

  removeLocation(id: string) {
    transaction(this.database, () => {
      this.database.prepare('DELETE FROM watch_locations WHERE id = ?').run(id)
    })
  }

  close() {
    this.database.close()
  }
}
