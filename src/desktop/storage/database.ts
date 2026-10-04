import { DatabaseSync } from 'node:sqlite'

const SCHEMA_VERSION = 2
const INITIAL_SCHEMA = `
  CREATE TABLE watch_locations (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    last_scan TEXT,
    scan_pending INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE application_state (id INTEGER PRIMARY KEY CHECK (id = 1), paused INTEGER NOT NULL DEFAULT 0, unclean INTEGER NOT NULL DEFAULT 0);
  INSERT INTO application_state (id) VALUES (1);
  CREATE TABLE verification_pending (watch_id TEXT PRIMARY KEY REFERENCES watch_locations(id) ON DELETE CASCADE);
  CREATE TABLE pending_changes (
    watch_id TEXT NOT NULL REFERENCES watch_locations(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    generation INTEGER NOT NULL,
    force INTEGER NOT NULL,
    PRIMARY KEY (watch_id, path)
  ) WITHOUT ROWID;
  CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    watch_id TEXT NOT NULL REFERENCES watch_locations(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ms REAL NOT NULL,
    UNIQUE (watch_id, path)
  );
  CREATE TABLE chunks (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    offset INTEGER NOT NULL,
    length INTEGER NOT NULL,
    hash BLOB NOT NULL,
    PRIMARY KEY (file_id, offset)
  ) WITHOUT ROWID;
  PRAGMA user_version = 2;
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
    const version = Number(database.prepare('PRAGMA user_version').get()!.user_version)
    if (version > SCHEMA_VERSION) throw new Error(`Unsupported database schema version ${version}.`)
    if (version !== 0 && version < SCHEMA_VERSION) throw new Error('Reset the development SQLite database before running this build.')
    if (version === 0) {
      if (!initialize) throw new Error('Database has not been initialized.')
      transaction(database, () => database.exec(INITIAL_SCHEMA))
    }
    database.exec('PRAGMA journal_mode = WAL')
    return database
  } catch (error) {
    database.close()
    throw error
  }
}
