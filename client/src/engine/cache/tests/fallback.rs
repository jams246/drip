use rusqlite::Connection;
use std::process::Command;

use super::support::Fixture;
use crate::source::SourceFile;

#[tokio::test]
async fn sqlite_capacity_failure_keeps_only_session_verified_memory() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    assert!(
        cache
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    let source = fixture.source();
    let (events, _received) = tokio::sync::mpsc::channel(8);
    let digest = source.hash(&events).await.unwrap();
    cache.state.lock().unwrap().fail_next_save_for_test();

    let warning = cache
        .remember(
            &fixture.selection,
            source.protocol_path(),
            source.observation(),
            source.volume_identity(),
            digest,
        )
        .await
        .unwrap();

    assert!(warning.contains("SQLITE_FULL"));
    assert_eq!(
        cache.get(&fixture.selection, &source, false).await.digest,
        Some(digest)
    );
    drop(cache);
    assert_miss_after_restart(&fixture, &source, true).await;
}

#[tokio::test]
async fn invalid_store_path_falls_back_without_touching_authoritative_files() {
    let fixture = Fixture::new();
    let settings = fixture.directory.join("drip.json");
    let credentials = fixture.directory.join("drip.credentials");
    std::fs::write(&settings, b"settings sentinel").unwrap();
    std::fs::write(&credentials, b"credentials sentinel").unwrap();
    std::fs::create_dir(fixture.database()).unwrap();
    let cache = fixture.cache();

    assert!(
        cache
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_some()
    );
    let source = fixture.source();
    let (events, _received) = tokio::sync::mpsc::channel(8);
    let digest = source.hash(&events).await.unwrap();
    assert!(
        cache
            .remember(
                &fixture.selection,
                source.protocol_path(),
                source.observation(),
                source.volume_identity(),
                digest,
            )
            .await
            .is_none()
    );
    assert_eq!(
        cache.get(&fixture.selection, &source, false).await.digest,
        Some(digest)
    );
    drop(cache);

    assert_miss_after_restart(&fixture, &source, false).await;
    assert_eq!(std::fs::read(settings).unwrap(), b"settings sentinel");
    assert_eq!(std::fs::read(credentials).unwrap(), b"credentials sentinel");
}

#[tokio::test]
async fn external_exclusive_lock_disables_backend_and_retains_new_memory_record() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    std::fs::write(&fixture.file, b"edited while store is locked").unwrap();
    let source = SourceFile::open(&fixture.file).unwrap();
    let lock = Connection::open(fixture.database()).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();

    let failed = cache.get(&fixture.selection, &source, false).await;
    assert!(failed.digest.is_none());
    assert!(failed.warning.is_some());
    let (events, _received) = tokio::sync::mpsc::channel(8);
    let digest = source.hash(&events).await.unwrap();
    assert!(
        cache
            .remember(
                &fixture.selection,
                source.protocol_path(),
                source.observation(),
                source.volume_identity(),
                digest,
            )
            .await
            .is_none()
    );
    assert_eq!(
        cache.get(&fixture.selection, &source, false).await.digest,
        Some(digest)
    );
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    drop(cache);
    assert_miss_after_restart(&fixture, &source, true).await;
}

#[tokio::test]
async fn read_only_store_permission_failure_uses_memory_only_fallback() {
    let fixture = Fixture::new();
    let first = fixture.cache();
    fixture.seed(&first, &fixture.selection).await;
    drop(first);
    let mut permissions = std::fs::metadata(fixture.database()).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(fixture.database(), permissions).unwrap();
    std::fs::write(&fixture.file, b"edited with read-only cache").unwrap();
    let source = fixture.source();
    let cache = fixture.cache();
    let warning = cache
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;

    assert!(warning.is_some());
    let (events, _received) = tokio::sync::mpsc::channel(8);
    let digest = source.hash(&events).await.unwrap();
    assert!(
        cache
            .remember(
                &fixture.selection,
                source.protocol_path(),
                source.observation(),
                source.volume_identity(),
                digest,
            )
            .await
            .is_none()
    );
    assert_eq!(
        cache.get(&fixture.selection, &source, false).await.digest,
        Some(digest)
    );
    drop(cache);
    let output = Command::new("attrib.exe")
        .args(["-R", fixture.database().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_miss_after_restart(&fixture, &source, true).await;
}

async fn assert_miss_after_restart(fixture: &Fixture, source: &SourceFile, available: bool) {
    let cache = fixture.cache();
    let warning = cache
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;
    assert_eq!(warning.is_none(), available, "warning={warning:?}");
    assert_eq!(
        cache.get(&fixture.selection, source, false).await.digest,
        None
    );
}
