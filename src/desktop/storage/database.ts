import { DatabaseSync } from 'node:sqlite'

const INITIAL_SCHEMA = `
  CREATE TABLE IF NOT EXISTS watch_locations (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    last_scan TEXT,
    scan_pending INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE IF NOT EXISTS application_state (id INTEGER PRIMARY KEY CHECK (id = 1), paused INTEGER NOT NULL DEFAULT 0, unclean INTEGER NOT NULL DEFAULT 0);
  INSERT OR IGNORE INTO application_state (id) VALUES (1);
  CREATE TABLE IF NOT EXISTS verification_pending (watch_id TEXT PRIMARY KEY REFERENCES watch_locations(id) ON DELETE CASCADE);
  CREATE TABLE IF NOT EXISTS pending_changes (
    watch_id TEXT NOT NULL REFERENCES watch_locations(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    generation INTEGER NOT NULL,
    force INTEGER NOT NULL,
    PRIMARY KEY (watch_id, path)
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    watch_id TEXT NOT NULL REFERENCES watch_locations(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ms REAL NOT NULL,
    source_path TEXT NOT NULL DEFAULT '',
    UNIQUE (watch_id, path)
  );
  CREATE TABLE IF NOT EXISTS chunks (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    offset INTEGER NOT NULL,
    length INTEGER NOT NULL,
    hash BLOB NOT NULL,
    PRIMARY KEY (file_id, offset)
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS sync_credentials (
    id INTEGER PRIMARY KEY CHECK (id = 1), url TEXT NOT NULL, device_id TEXT NOT NULL,
    secret TEXT NOT NULL, name TEXT NOT NULL, token TEXT NOT NULL, confirmed INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE IF NOT EXISTS sync_state (id INTEGER PRIMARY KEY CHECK (id = 1), generation INTEGER NOT NULL DEFAULT 0);
  INSERT OR IGNORE INTO sync_state (id) VALUES (1);
  CREATE TABLE IF NOT EXISTS sync_roots (
    watch_id TEXT PRIMARY KEY, root_id TEXT NOT NULL UNIQUE, path TEXT NOT NULL, name TEXT NOT NULL,
    kind TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1, registered INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 0, coverage INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE IF NOT EXISTS sync_pending (
    root_id TEXT NOT NULL, path_key TEXT NOT NULL, generation INTEGER NOT NULL,
    source_path TEXT NOT NULL, change TEXT NOT NULL, PRIMARY KEY (root_id, path_key)
  ) WITHOUT ROWID;
  CREATE TABLE IF NOT EXISTS sync_operations (
    root_id TEXT PRIMARY KEY, operation_id TEXT NOT NULL UNIQUE, base_revision INTEGER NOT NULL,
    path_key TEXT NOT NULL, generation INTEGER NOT NULL, source_path TEXT NOT NULL,
    change TEXT NOT NULL, abort_requested INTEGER NOT NULL DEFAULT 0
  );
`

export function transaction(database: DatabaseSync, operation: () => void) {
  // Acquire the write lock before running the transactional statements.
  database.exec('BEGIN IMMEDIATE')
  try {
    operation()
    database.exec('COMMIT')
  } catch (error) {
    if (database.isTransaction) database.exec('ROLLBACK')
    throw error
  }
}

export function openDatabase(path: string, initialize = true, timeout = 0): DatabaseSync {
  const database = new DatabaseSync(path, { timeout })
  try {
    // Native constructor timeout zero retains its default; set it explicitly for responsive host retries.
    database.exec(`PRAGMA busy_timeout = ${timeout}`)
    database.exec('PRAGMA foreign_keys = ON')
    if (initialize) transaction(database, () => database.exec(INITIAL_SCHEMA))
    database.exec('PRAGMA journal_mode = WAL')
    return database
  } catch (error) {
    database.close()
    throw error
  }
}
