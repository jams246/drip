import type { DatabaseSync, StatementSync } from 'node:sqlite'
import { Buffer } from 'node:buffer'
import { openDatabase, transaction } from './database'

const WORKER_WRITE_TIMEOUT_MS = 5000

export function normalizeFileId(path: string): string {
  let id = path.split('\\').join('/').toLowerCase()
  while (id.endsWith('/')) id = id.slice(0, -1)
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
  private pruningSafe = true

  constructor(
    databasePath: string,
    private readonly watchId: string
  ) {
    this.database = openDatabase(databasePath, false, WORKER_WRITE_TIMEOUT_MS)
    try {
      this.database.exec(`PRAGMA temp_store = FILE;
        CREATE TEMP TABLE staged_chunks (offset INTEGER PRIMARY KEY, length INTEGER NOT NULL, hash BLOB NOT NULL);
        CREATE TEMP TABLE seen_files (id TEXT PRIMARY KEY);`)
      this.insertChunk = this.database.prepare('INSERT INTO staged_chunks (offset, length, hash) VALUES (?, ?, ?)')
    } catch (error) {
      this.database.close()
      throw error
    }
  }

  beginFile(path: string) {
    try {
      this.abortFile()
      this.database.prepare('INSERT OR IGNORE INTO seen_files (id) VALUES (?)').run(normalizeFileId(path))
      this.database.exec('DELETE FROM staged_chunks; BEGIN')
    } catch (error) {
      this.pruningSafe = false
      throw error
    }
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
        this.database
          .prepare(`INSERT INTO files (watch_id, path, size, modified_ms) VALUES (?, ?, ?, ?)
            ON CONFLICT(watch_id, path) DO UPDATE SET size = excluded.size, modified_ms = excluded.modified_ms`)
          .run(this.watchId, normalizedPath, size, modifiedMs)
        const fileId = Number(this.database.prepare('SELECT id FROM files WHERE watch_id = ? AND path = ?').get(this.watchId, normalizedPath)!.id)
        this.database.prepare('DELETE FROM chunks WHERE file_id = ?').run(fileId)
        this.database.prepare('INSERT INTO chunks (file_id, offset, length, hash) SELECT ?, offset, length, hash FROM staged_chunks').run(fileId)
      })
    } catch (error) {
      this.abortFile()
      throw error
    }
  }

  abortFile() {
    if (this.database.isTransaction) this.database.exec('ROLLBACK')
  }

  finishFolder(prune: boolean) {
    if (!prune || !this.pruningSafe) return
    transaction(this.database, () => {
      this.database.prepare('DELETE FROM files WHERE watch_id = ? AND path NOT IN (SELECT id FROM seen_files)').run(this.watchId)
    })
  }

  readChunks(path: string): { offset: number; length: number; hash: string }[] {
    return this.database
      .prepare(`SELECT offset, length, hash FROM chunks
        WHERE file_id = (SELECT id FROM files WHERE watch_id = ? AND path = ?) ORDER BY offset`)
      .all(this.watchId, normalizeFileId(path))
      .map((row) => ({
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
