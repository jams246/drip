import { DatabaseSync } from 'node:sqlite'

const WRITE_TIMEOUT_MS = 5000
const SCHEMA = `
  CREATE TABLE IF NOT EXISTS device_fences (device_id TEXT PRIMARY KEY);
  CREATE TABLE IF NOT EXISTS roots (
    id TEXT PRIMARY KEY, device_id TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0, retired INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE IF NOT EXISTS heads (
    root_id TEXT NOT NULL REFERENCES roots(id), path_key TEXT NOT NULL, manifest TEXT NOT NULL,
    PRIMARY KEY(root_id, path_key)
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS chunks (hash TEXT PRIMARY KEY, length INTEGER NOT NULL, deleting INTEGER NOT NULL DEFAULT 0);
  CREATE TABLE IF NOT EXISTS head_chunks (
    root_id TEXT NOT NULL, path_key TEXT NOT NULL, hash TEXT NOT NULL REFERENCES chunks(hash),
    PRIMARY KEY(root_id, path_key, hash), FOREIGN KEY(root_id,path_key) REFERENCES heads(root_id,path_key) ON DELETE CASCADE
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY, root_id TEXT NOT NULL REFERENCES roots(id), base_revision INTEGER NOT NULL,
    payload TEXT NOT NULL, status TEXT NOT NULL, revision INTEGER NOT NULL, expires_at INTEGER NOT NULL
  );
  CREATE UNIQUE INDEX IF NOT EXISTS active_root_operation ON operations(root_id) WHERE status IN ('offered','publishing');
  CREATE TABLE IF NOT EXISTS operation_fingerprints (
    operation_id TEXT PRIMARY KEY, root_id TEXT NOT NULL REFERENCES roots(id), fingerprint TEXT NOT NULL
  );
  CREATE TABLE IF NOT EXISTS operation_pins (
    operation_id TEXT NOT NULL REFERENCES operations(id) ON DELETE CASCADE, hash TEXT NOT NULL, length INTEGER NOT NULL,
    PRIMARY KEY(operation_id,hash)
  ) WITHOUT ROWID;
  CREATE INDEX IF NOT EXISTS head_chunks_hash ON head_chunks(hash);
  CREATE INDEX IF NOT EXISTS operation_pins_hash ON operation_pins(hash);
  CREATE INDEX IF NOT EXISTS operation_fingerprints_root ON operation_fingerprints(root_id);
  CREATE TABLE IF NOT EXISTS publication_journal (operation_id TEXT PRIMARY KEY REFERENCES operations(id) ON DELETE CASCADE);
  PRAGMA user_version = 1;
`

export function openSyncDatabase(path: string) {
  const database = new DatabaseSync(path, { timeout: WRITE_TIMEOUT_MS })
  database.exec('PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL')
  const version = Number(database.prepare('PRAGMA user_version').get()!.user_version)
  if (version > 1) {
    database.close()
    throw new Error('Unsupported synchronization database version.')
  }
  database.exec(SCHEMA)
  return database
}

export function transaction<T>(database: DatabaseSync, operation: () => T): T {
  database.exec('BEGIN IMMEDIATE')
  try {
    const result = operation()
    database.exec('COMMIT')
    return result
  } catch (error) {
    database.exec('ROLLBACK')
    throw error
  }
}
