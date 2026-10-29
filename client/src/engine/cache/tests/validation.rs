use rusqlite::Connection;

use super::support::Fixture;
use crate::source::SourceFile;

#[tokio::test]
async fn changed_observation_fields_and_invalid_digest_miss() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    drop(cache);

    for update in [
        "UPDATE cached_files SET volume_identity = volume_identity || '-changed'",
        "UPDATE cached_files SET volume_serial = X'FFFFFFFFFFFFFFFF'",
        "UPDATE cached_files SET file_id = X'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF'",
        "UPDATE cached_files SET file_size = file_size + 1",
        "UPDATE cached_files SET last_write_seconds = last_write_seconds + 1",
        "UPDATE cached_files SET last_write_nanos = last_write_nanos + 1",
        "UPDATE cached_files SET change_time = change_time + 1",
    ] {
        restore_record(&fixture).await;
        Connection::open(fixture.database())
            .unwrap()
            .execute(update, [])
            .unwrap();
        assert_miss(&fixture).await;
    }

    restore_record(&fixture).await;
    let connection = Connection::open(fixture.database()).unwrap();
    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON; UPDATE cached_files SET digest = X'01';",
        )
        .unwrap();
    assert_miss(&fixture).await;
}

#[tokio::test]
async fn same_size_edit_with_restored_write_time_misses_change_time() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    drop(cache);
    let original = std::fs::metadata(&fixture.file)
        .unwrap()
        .modified()
        .unwrap();
    std::fs::write(&fixture.file, b"new bytes").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&fixture.file)
        .unwrap()
        .set_modified(original)
        .unwrap();
    assert_miss(&fixture).await;
}

async fn restore_record(fixture: &Fixture) {
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
}

async fn assert_miss(fixture: &Fixture) {
    let cache = fixture.cache();
    assert!(
        cache
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    let source = SourceFile::open(&fixture.file).unwrap();
    source.verify().await.unwrap();
    assert_eq!(
        cache.get(&fixture.selection, &source, false).await.digest,
        None
    );
    let rows: i64 = Connection::open(fixture.database())
        .unwrap()
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
