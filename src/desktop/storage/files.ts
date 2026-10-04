import type { DatabaseSync, StatementSync } from 'node:sqlite'
import { Buffer } from 'node:buffer'
import { openDatabase, transaction } from './database'

const WORKER_WRITE_TIMEOUT_MS = 5000
// Preserve drive roots like c:/ when trimming trailing slashes.
const DRIVE_ROOT_ID_LENGTH = 3

export function normalizeFileId(path: string): string {
  let id = path.split('\\').join('/').toLowerCase()
  while (id.endsWith('/') && id.length > DRIVE_ROOT_ID_LENGTH) id = id.slice(0, -1)
  return id
}

export function readFileChunks(databasePath: string, watchId: string, path: string): { offset: number; length: number; hash: string }[] {
  const store = new FileStore(databasePath, watchId)
  try {
    return store.readChunks(path)
  } finally {
    store.close()
  }
}

export class FileStore {
  private readonly database: DatabaseSync
  private readonly insertChunk: StatementSync
  private readonly insertSeen: StatementSync
  private readonly selectMetadata: StatementSync
  private readonly deleteFile: StatementSync
  private readonly upsertFile: StatementSync
  private readonly selectFileId: StatementSync
  private readonly deleteChunks: StatementSync
  private readonly insertChunks: StatementSync
  private readonly selectUnseen: StatementSync
  private readonly selectChunks: StatementSync

  constructor(
    databasePath: string,
    private watchId: string
  ) {
    this.database = openDatabase(databasePath, false, WORKER_WRITE_TIMEOUT_MS)
    try {
      this.database.exec(`PRAGMA temp_store = FILE;
        CREATE TEMP TABLE staged_chunks (offset INTEGER PRIMARY KEY, length INTEGER NOT NULL, hash BLOB NOT NULL);
        CREATE TEMP TABLE seen_files (id TEXT PRIMARY KEY);`)
      this.insertChunk = this.database.prepare('INSERT INTO staged_chunks (offset, length, hash) VALUES (?, ?, ?)')
      this.insertSeen = this.database.prepare('INSERT OR IGNORE INTO seen_files (id) VALUES (?)')
      this.selectMetadata = this.database.prepare('SELECT size, modified_ms FROM files WHERE watch_id = ? AND path = ?')
      this.deleteFile = this.database.prepare('DELETE FROM files WHERE watch_id = ? AND path = ?')
      this.upsertFile = this.database.prepare(`INSERT INTO files (watch_id, path, size, modified_ms) VALUES (?, ?, ?, ?)
        ON CONFLICT(watch_id, path) DO UPDATE SET size = excluded.size, modified_ms = excluded.modified_ms`)
      this.selectFileId = this.database.prepare('SELECT id FROM files WHERE watch_id = ? AND path = ?')
      this.deleteChunks = this.database.prepare('DELETE FROM chunks WHERE file_id = ?')
      this.insertChunks = this.database.prepare('INSERT INTO chunks (file_id, offset, length, hash) SELECT ?, offset, length, hash FROM staged_chunks')
      this.selectUnseen = this.database.prepare(`SELECT path FROM files WHERE watch_id = ? AND (path = ? OR substr(path, 1, ?) = ?)
        AND path > ? AND path NOT IN (SELECT id FROM seen_files) ORDER BY path LIMIT ?`)
      this.selectChunks = this.database.prepare(`SELECT offset, length, hash FROM chunks
        WHERE file_id = (SELECT id FROM files WHERE watch_id = ? AND path = ?) ORDER BY offset`)
    } catch (error) {
      this.database.close()
      throw error
    }
  }

  reset(watchId: string) {
    this.abortFile()
    this.database.exec('DELETE FROM staged_chunks; DELETE FROM seen_files')
    this.watchId = watchId
  }

  beginFile(path: string) {
    this.abortFile()
    this.markSeen(path)
    this.database.exec('DELETE FROM staged_chunks; BEGIN')
  }

  markSeen(path: string) {
    this.insertSeen.run(normalizeFileId(path))
  }

  needsHash(path: string, size: number, modifiedMs: number): boolean {
    const saved = this.selectMetadata.get(this.watchId, normalizeFileId(path))
    return !saved || Number(saved.size) !== size || Number(saved.modified_ms) !== modifiedMs
  }

  removeFile(path: string) {
    transaction(this.database, () => {
      this.deleteFile.run(this.watchId, normalizeFileId(path))
    })
  }

  stageChunk(offset: number, length: number, hash: string) {
    this.insertChunk.run(offset, length, Buffer.from(hash, 'hex'))
  }

  commitFile(path: string, size: number, modifiedMs: number) {
    const normalizedPath = normalizeFileId(path)
    try {
      // Keep durable reads out of TEMP staging so host edits never invalidate a WAL read snapshot.
      this.database.exec('COMMIT')
      transaction(this.database, () => {
        this.upsertFile.run(this.watchId, normalizedPath, size, modifiedMs)
        const fileId = Number(this.selectFileId.get(this.watchId, normalizedPath)!.id)
        this.deleteChunks.run(fileId)
        this.insertChunks.run(fileId)
      })
    } catch (error) {
      this.abortFile()
      throw error
    }
  }

  abortFile() {
    if (this.database.isTransaction) this.database.exec('ROLLBACK')
  }

  unseenPaths(scope: string, after: string, limit: number): string[] {
    const normalizedScope = normalizeFileId(scope)
    const prefix = normalizedScope.endsWith('/') ? normalizedScope : `${normalizedScope}/`
    return this.selectUnseen.all(this.watchId, normalizedScope, prefix.length, prefix, after, limit).map((row) => String(row.path))
  }

  readChunks(path: string): { offset: number; length: number; hash: string }[] {
    return this.selectChunks.all(this.watchId, normalizeFileId(path)).map((row) => ({
      offset: Number(row.offset),
      length: Number(row.length),
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- SQLite BLOB columns return byte arrays.
      hash: Buffer.from(row.hash as Uint8Array).toString('hex')
    }))
  }

  close() {
    try {
      this.abortFile()
    } finally {
      this.database.close()
    }
  }
}
