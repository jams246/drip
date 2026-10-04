import { DatabaseSync } from 'node:sqlite'

const WRITE_TIMEOUT_MS = 5000
const SCHEMA = `
  CREATE TABLE IF NOT EXISTS device_fences (device_id TEXT PRIMARY KEY);
  CREATE TABLE IF NOT EXISTS roots (
    id TEXT PRIMARY KEY, device_id TEXT NOT NULL UNIQUE, name TEXT NOT NULL, kind TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0, retired INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE IF NOT EXISTS heads (
    root_id TEXT NOT NULL REFERENCES roots(id), path_key TEXT NOT NULL, manifest TEXT NOT NULL,
    PRIMARY KEY(root_id, path_key)
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS head_regions (
    root_id TEXT NOT NULL, path_key TEXT NOT NULL, offset INTEGER NOT NULL, length INTEGER NOT NULL, hash TEXT NOT NULL,
    PRIMARY KEY(root_id, path_key, offset), FOREIGN KEY(root_id,path_key) REFERENCES heads(root_id,path_key) ON DELETE CASCADE
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS operations (
    id TEXT PRIMARY KEY, root_id TEXT NOT NULL REFERENCES roots(id), base_revision INTEGER NOT NULL,
    payload TEXT NOT NULL, status TEXT NOT NULL, revision INTEGER NOT NULL, expires_at INTEGER NOT NULL,
    plan_complete INTEGER NOT NULL, stage_ready INTEGER NOT NULL, metadata_only INTEGER NOT NULL, basis_manifest TEXT,
    content_verified INTEGER NOT NULL DEFAULT 0
  );
  CREATE UNIQUE INDEX IF NOT EXISTS active_root_operation ON operations(root_id) WHERE status IN ('offered','publishing');
  CREATE TABLE IF NOT EXISTS operation_fingerprints (
    operation_id TEXT PRIMARY KEY, root_id TEXT NOT NULL REFERENCES roots(id), fingerprint TEXT NOT NULL
  );
  CREATE TABLE IF NOT EXISTS operation_regions (
    operation_id TEXT NOT NULL REFERENCES operations(id) ON DELETE CASCADE, offset INTEGER NOT NULL,
    length INTEGER NOT NULL, hash TEXT NOT NULL, basis_offset INTEGER, uploaded INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(operation_id,offset)
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS operation_moves (
    operation_id TEXT NOT NULL REFERENCES operations(id) ON DELETE CASCADE, position INTEGER NOT NULL, phase INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(operation_id,position)
  ) WITHOUT ROWID;
  CREATE INDEX IF NOT EXISTS head_regions_hash ON head_regions(root_id,path_key,hash,length);
  CREATE INDEX IF NOT EXISTS operation_fingerprints_root ON operation_fingerprints(root_id);
  CREATE TABLE IF NOT EXISTS publication_journal (operation_id TEXT PRIMARY KEY REFERENCES operations(id) ON DELETE CASCADE);
  PRAGMA user_version = 2;
`

export function openSyncDatabase(path: string) {
  const database = new DatabaseSync(path, { timeout: WRITE_TIMEOUT_MS })
  database.exec('PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL')
  const version = Number(database.prepare('PRAGMA user_version').get()!.user_version)
  if (version !== 0 && version !== 2) {
    database.close()
    throw new Error('The chunk synchronization database is unsupported. Use a fresh deployment.')
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
