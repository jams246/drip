use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use drip_core::SourcePath;
use rusqlite::Connection;

use crate::Store;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn legacy_selection_rows_migrate_to_file_roots() {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".verification/phase-02/schema-migrations");
    std::fs::create_dir_all(&parent).unwrap();
    let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = parent.join(format!("{}-{suffix}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let state = root.join("state");
    std::fs::create_dir(&state).unwrap();
    let database = state.join("drip.sqlite3");
    let source =
        serde_json::to_string(&SourcePath::new('C', vec!["legacy.txt".to_owned()]).unwrap())
            .unwrap();
    let credential = "c".repeat(64);
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE devices (
                 pc_id TEXT PRIMARY KEY, label TEXT NOT NULL, mirror_directory TEXT NOT NULL UNIQUE,
                 credential_digest TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL, revoked_at INTEGER
             );
             CREATE TABLE device_state (
                 pc_id TEXT PRIMARY KEY REFERENCES devices(pc_id), generation INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE selections (
                 pc_id TEXT NOT NULL REFERENCES device_state(pc_id), selection_id TEXT NOT NULL,
                 source_json TEXT NOT NULL, active INTEGER NOT NULL, PRIMARY KEY(pc_id, selection_id)
             );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO devices VALUES (?1, 'legacy', 'legacy-pc', ?2, 0, NULL)",
            (
                "a".repeat(32),
                blake3::hash(credential.as_bytes()).to_hex().to_string(),
            ),
        )
        .unwrap();
    connection
        .execute("INSERT INTO device_state VALUES (?1, 0)", ["a".repeat(32)])
        .unwrap();
    connection
        .execute(
            "INSERT INTO selections VALUES (?1, ?2, ?3, 1)",
            ("a".repeat(32), "b".repeat(32), source),
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&state, root.join("mirror")).unwrap();
    let current = store.get_selections(&credential).unwrap();
    assert_eq!(
        current.selections[0].root_kind,
        drip_core::protocol::EntryKind::File
    );
    assert_eq!(
        current.selections[0].source,
        SourcePath::new('C', vec!["legacy.txt".to_owned()]).unwrap()
    );
}
