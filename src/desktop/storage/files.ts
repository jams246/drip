import type { DatabaseSync, StatementSync } from 'node:sqlite'
import type { MirrorEntry, RegionFingerprint } from '../../protocol/sync'
import { openDatabase, transaction } from './database'
import { catalogueChange, catalogueEntry } from './sync-outbox'

const WORKER_WRITE_TIMEOUT_MS = 5000
const DRIVE_ROOT_ID_LENGTH = 3

export function normalizeFileId(path: string): string {
  let id = path.split('\\').join('/').toLowerCase()
  while (id.endsWith('/') && id.length > DRIVE_ROOT_ID_LENGTH) id = id.slice(0, -1)
  return id
}

export class FileStore {
  private readonly database: DatabaseSync
  private readonly insertRegion: StatementSync
  private readonly insertSeen: StatementSync
  private readonly selectMetadata: StatementSync
  private readonly upsertFile: StatementSync
  private readonly selectUnseen: StatementSync

  constructor(
    databasePath: string,
    private watchId: string
  ) {
    this.database = openDatabase(databasePath, false, WORKER_WRITE_TIMEOUT_MS)
    try {
      this.database.exec(`PRAGMA temp_store = FILE;
        CREATE TEMP TABLE staged_regions (offset INTEGER PRIMARY KEY, length INTEGER NOT NULL, hash TEXT NOT NULL);
        CREATE TEMP TABLE seen_files (id TEXT PRIMARY KEY);`)
      this.insertRegion = this.database.prepare('INSERT INTO staged_regions VALUES (?,?,?)')
      this.insertSeen = this.database.prepare('INSERT OR IGNORE INTO seen_files VALUES (?)')
      this.selectMetadata = this.database.prepare('SELECT kind,size,modified_ms,source_path,hash FROM files WHERE watch_id=? AND path=?')
      this.upsertFile = this.database.prepare(`INSERT INTO files (watch_id,path,kind,size,modified_ms,source_path,hash) VALUES (?,?,?,?,?,?,?)
        ON CONFLICT(watch_id,path) DO UPDATE SET kind=excluded.kind,size=excluded.size,modified_ms=excluded.modified_ms,
        source_path=excluded.source_path,hash=excluded.hash`)
      this.selectUnseen = this.database.prepare(`SELECT path FROM files WHERE watch_id=? AND (path=? OR substr(path,1,?)=?)
        AND path>? AND path NOT IN (SELECT id FROM seen_files) ORDER BY path LIMIT ?`)
    } catch (error) {
      this.database.close()
      throw error
    }
  }

  reset(watchId: string) {
    this.abortFile()
    this.database.exec('DELETE FROM staged_regions; DELETE FROM seen_files')
    this.watchId = watchId
  }

  beginFile(path: string) {
    this.abortFile()
    this.markSeen(path)
    this.database.exec('DELETE FROM staged_regions; BEGIN')
  }

  markSeen(path: string) {
    this.insertSeen.run(normalizeFileId(path))
  }

  needsHash(path: string, size: number, modifiedMs: number): boolean {
    const saved = this.selectMetadata.get(this.watchId, normalizeFileId(path))
    return !saved || saved.kind !== 'file' || Number(saved.size) !== size || Number(saved.modified_ms) !== modifiedMs
  }

  private previousEntry(path: string): MirrorEntry | undefined {
    const entry = catalogueEntry(this.database, this.watchId, path)
    if (entry?.change.kind === 'upsert') return entry.change.entry
    return undefined
  }

  removeFile(path: string) {
    const normalized = normalizeFileId(path)
    transaction(this.database, () => {
      catalogueChange(this.database, this.watchId, normalized, true)
      this.database.prepare('DELETE FROM files WHERE watch_id=? AND path=?').run(this.watchId, normalized)
    })
  }

  stageRegion(offset: number, length: number, hash: string) {
    this.insertRegion.run(offset, length, hash)
  }

  commitFile(path: string, size: number, modifiedMs: number, hash: string) {
    const normalized = normalizeFileId(path)
    try {
      this.database.exec('COMMIT')
      transaction(this.database, () => {
        const previous = this.previousEntry(normalized)
        this.upsertFile.run(this.watchId, normalized, 'file', size, modifiedMs, path, hash)
        const fileId = this.database.prepare('SELECT id FROM files WHERE watch_id=? AND path=?').get(this.watchId, normalized)!.id
        this.database.prepare('DELETE FROM file_regions WHERE file_id=?').run(fileId)
        this.database.prepare('INSERT INTO file_regions SELECT ?,offset,length,hash FROM staged_regions').run(fileId)
        catalogueChange(this.database, this.watchId, normalized, false, previous)
      })
    } catch (error) {
      this.abortFile()
      throw error
    }
  }

  observeDirectory(path: string) {
    this.markSeen(path)
    const normalized = normalizeFileId(path)
    const saved = this.selectMetadata.get(this.watchId, normalized)
    if (saved?.kind === 'directory' && saved.source_path === path) return
    transaction(this.database, () => {
      const previous = this.previousEntry(normalized)
      this.upsertFile.run(this.watchId, normalized, 'directory', 0, 0, path, '')
      this.database.prepare('DELETE FROM file_regions WHERE file_id=(SELECT id FROM files WHERE watch_id=? AND path=?)').run(this.watchId, normalized)
      catalogueChange(this.database, this.watchId, normalized, false, previous)
    })
  }

  abortFile() {
    if (this.database.isTransaction) this.database.exec('ROLLBACK')
  }

  observeFile(path: string) {
    const normalized = normalizeFileId(path)
    const saved = this.selectMetadata.get(this.watchId, normalized)
    if (!saved || saved.source_path === path) return
    transaction(this.database, () => {
      const previous = this.previousEntry(normalized)
      this.database.prepare('UPDATE files SET source_path=? WHERE watch_id=? AND path=?').run(path, this.watchId, normalized)
      catalogueChange(this.database, this.watchId, normalized, false, previous)
    })
  }

  coverage(safe: boolean) {
    this.database.prepare('UPDATE watch_locations SET coverage=? WHERE id=?').run(safe ? 1 : 0, this.watchId)
  }

  unseenPaths(scope: string, after: string, limit: number): string[] {
    const normalized = normalizeFileId(scope)
    const prefix = normalized.endsWith('/') ? normalized : normalized + '/'
    return this.selectUnseen.all(this.watchId, normalized, prefix.length, prefix, after, limit).map((row) => String(row.path))
  }

  readRegions(path: string): RegionFingerprint[] {
    return this.database
      .prepare(`SELECT offset,length,hash FROM file_regions
      WHERE file_id=(SELECT id FROM files WHERE watch_id=? AND path=?) ORDER BY offset`)
      .all(this.watchId, normalizeFileId(path))
      .map((row) => ({ offset: Number(row.offset), length: Number(row.length), hash: String(row.hash) }))
  }

  close() {
    try {
      this.abortFile()
    } finally {
      this.database.close()
    }
  }
}
