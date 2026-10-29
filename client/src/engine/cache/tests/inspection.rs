use std::{fs, path::Path, process::Command};

use rusqlite::Connection;

use super::support::Fixture;

#[tokio::test]
async fn all_sidecar_paths_are_rejected_before_sqlite_opens_them() {
    for suffix in ["-journal", "-wal", "-shm"] {
        let fixture = Fixture::new();
        let first = fixture.cache();
        fixture.seed(&first, &fixture.selection).await;
        drop(first);
        let sidecar = fixture
            .directory
            .join(format!("drip.state.sqlite3{suffix}"));
        if sidecar.is_file() {
            fs::remove_file(&sidecar).unwrap();
        }
        fs::create_dir(&sidecar).unwrap();
        let sentinel = sidecar.join("sentinel");
        fs::write(&sentinel, b"sidecar target").unwrap();

        let warning = fixture
            .cache()
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .unwrap();

        assert!(warning.contains("unsafe"), "{suffix}: {warning}");
        assert_eq!(fs::read(sentinel).unwrap(), b"sidecar target");
    }
}

#[tokio::test]
async fn future_schema_and_hot_journal_remain_byte_for_byte_intact() {
    let fixture = seeded_fixture().await;
    Connection::open(fixture.database())
        .unwrap()
        .pragma_update(None, "user_version", 2)
        .unwrap();
    crash_writer(&fixture.database(), "journal");

    assert_preserved(&fixture, &["", "-journal"]).await;
}

#[tokio::test]
async fn future_fingerprint_and_hot_journal_remain_byte_for_byte_intact() {
    let fixture = seeded_fixture().await;
    Connection::open(fixture.database())
        .unwrap()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE state_binding SET fingerprint_format = 'future-v9';",
        )
        .unwrap();
    crash_writer(&fixture.database(), "journal");

    assert_preserved(&fixture, &["", "-journal"]).await;
}

#[tokio::test]
async fn future_version_in_wal_remains_intact_without_checkpointing() {
    let fixture = seeded_fixture().await;
    crash_writer(&fixture.database(), "wal");

    assert_preserved(&fixture, &["", "-wal", "-shm"]).await;
}

#[tokio::test]
async fn supported_hot_journal_recovers_before_reusing_verified_records() {
    let fixture = seeded_fixture().await;
    crash_writer(&fixture.database(), "journal");
    let cache = fixture.cache();

    assert!(
        cache
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    assert_eq!(
        cache
            .get(&fixture.selection, &fixture.source(), false)
            .await
            .digest,
        Some(drip_core::Blake3Digest::new(
            *blake3::hash(b"old bytes").as_bytes()
        ))
    );
    let size: i64 = Connection::open(fixture.database())
        .unwrap()
        .query_row(
            "SELECT length(payload) FROM inspection_probe LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(size, 3000);
}

#[tokio::test]
async fn compatibility_inspection_encodes_spaces_hashes_and_percent_signs() {
    let fixture = Fixture::new();
    let directory = fixture.directory.join("cache # 100%");
    fs::create_dir(&directory).unwrap();
    let store = crate::settings::SettingsStore::open(&directory);
    let cache = crate::engine::cache::FileCache::beside_settings(&store);
    let digest = fixture.seed(&cache, &fixture.selection).await;
    drop(cache);

    let reopened = crate::engine::cache::FileCache::beside_settings(&store);
    assert!(
        reopened
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    assert_eq!(
        reopened
            .get(&fixture.selection, &fixture.source(), false)
            .await
            .digest,
        Some(digest)
    );
}

async fn seeded_fixture() -> Fixture {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    drop(cache);
    Connection::open(fixture.database())
        .unwrap()
        .execute_batch(
            "CREATE TABLE inspection_probe (payload BLOB NOT NULL);
             INSERT INTO inspection_probe SELECT zeroblob(3000) FROM cached_files;
             INSERT INTO inspection_probe SELECT payload FROM inspection_probe;
             INSERT INTO inspection_probe SELECT payload FROM inspection_probe;
             INSERT INTO inspection_probe SELECT payload FROM inspection_probe;",
        )
        .unwrap();
    fixture
}

async fn assert_preserved(fixture: &Fixture, suffixes: &[&str]) {
    let originals: Vec<_> = suffixes
        .iter()
        .map(|suffix| {
            let path = fixture
                .directory
                .join(format!("drip.state.sqlite3{suffix}"));
            let bytes = fs::read(&path).unwrap();
            assert!(!bytes.is_empty());
            (path, bytes)
        })
        .collect();
    let warning = fixture
        .cache()
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;
    assert!(warning.is_some());
    for (path, bytes) in originals {
        assert!(
            fs::read(&path).unwrap() == bytes,
            "{} changed",
            path.display()
        );
    }
}

fn crash_writer(path: &Path, mode: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "engine::cache::tests::inspection::crash_writer_child",
            "--exact",
            "--ignored",
        ])
        .env("DRIP_CACHE_CRASH_DATABASE", path)
        .env("DRIP_CACHE_CRASH_MODE", mode)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(89), "{output:?}");
}

#[test]
#[ignore = "deterministic subprocess checkpoint for cache inspection tests"]
fn crash_writer_child() {
    let path = std::path::PathBuf::from(std::env::var_os("DRIP_CACHE_CRASH_DATABASE").unwrap());
    let allowed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/persistent-cache/cache-tests");
    assert!(path.starts_with(allowed));
    let connection = Connection::open(path).unwrap();
    if std::env::var("DRIP_CACHE_CRASH_MODE").unwrap() == "wal" {
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0; PRAGMA user_version = 2;")
            .unwrap();
    } else {
        connection
            .execute_batch("PRAGMA cache_size = 1; BEGIN IMMEDIATE; UPDATE inspection_probe SET payload = zeroblob(4000); UPDATE cached_files SET digest = zeroblob(32);")
            .unwrap();
        connection.cache_flush().unwrap();
    }
    std::process::exit(89);
}
