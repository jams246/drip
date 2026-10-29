use rusqlite::Connection;

use super::support::Fixture;
use crate::{
    engine::cache::{database::Binding, state::CacheState},
    source::SourceFile,
};

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

    for update in [
        "UPDATE cached_files SET volume_serial = X'01'",
        "UPDATE cached_files SET file_id = X'01'",
        "UPDATE cached_files SET file_size = -1",
        "UPDATE cached_files SET last_write_nanos = 1000000000",
        "UPDATE cached_files SET change_time = 0",
        "UPDATE cached_files SET digest = X'01'",
        "UPDATE cached_files SET verified_seconds = -1",
        "UPDATE cached_files SET verified_nanos = 1000000000",
    ] {
        restore_record(&fixture).await;
        Connection::open(fixture.database())
            .unwrap()
            .execute_batch(&format!("PRAGMA ignore_check_constraints = ON; {update};"))
            .unwrap();
        assert_miss(&fixture).await;
    }
}

#[tokio::test]
async fn unavailable_change_time_never_uses_or_deletes_a_saved_digest() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    drop(cache);

    let source = fixture.source();
    let mut observation = source.observation();
    observation.change_time = 0;
    let mut state = CacheState::default();
    assert!(
        state
            .reconcile(
                Some(&fixture.database()),
                Binding::from(&fixture.pairing),
                vec![fixture.selection.clone()],
            )
            .is_none()
    );
    let path = source.protocol_path();
    let result = state.get(
        &fixture.selection,
        path,
        &serde_json::to_string(path).unwrap(),
        observation,
        source.volume_identity(),
    );

    assert!(result.digest.is_none());
    let rows: i64 = Connection::open(fixture.database())
        .unwrap()
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn row_path_outside_its_saved_selection_is_discarded() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    drop(cache);
    let outside = fixture.directory.join("outside.txt");
    std::fs::write(&outside, b"outside selection").unwrap();
    let outside_source = SourceFile::open(&outside).unwrap();
    Connection::open(fixture.database())
        .unwrap()
        .execute(
            "UPDATE cached_files SET source_json = ?1",
            [serde_json::to_string(outside_source.protocol_path()).unwrap()],
        )
        .unwrap();

    let cache = fixture.cache();
    assert!(
        cache
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    assert_eq!(
        cache
            .get(&fixture.selection, &outside_source, false)
            .await
            .digest,
        None
    );
    let rows: i64 = Connection::open(fixture.database())
        .unwrap()
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
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
