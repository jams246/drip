use rusqlite::Connection;

use super::support::Fixture;
use crate::engine::cache::database::{Binding, CacheDatabase};

#[tokio::test]
async fn changed_selection_invalidates_only_its_rows() {
    let fixture = Fixture::new();
    let mut other = fixture.selection.clone();
    other.id = crate::source::random_id().unwrap();
    let cache = fixture.cache();
    assert!(
        cache
            .reconcile(
                &fixture.pairing,
                &[fixture.selection.clone(), other.clone()]
            )
            .await
            .is_none()
    );
    let source = fixture.source();
    let (events, _received) = tokio::sync::mpsc::channel(8);
    let digest = source.hash(&events).await.unwrap();
    for selection in [&fixture.selection, &other] {
        assert!(
            cache
                .remember(
                    selection,
                    source.protocol_path(),
                    source.observation(),
                    source.volume_identity(),
                    digest,
                )
                .await
                .is_none()
        );
    }
    drop(cache);

    let mut inactive = fixture.selection.clone();
    inactive.active = false;
    let reopened = fixture.cache();
    assert!(
        reopened
            .reconcile(&fixture.pairing, &[inactive.clone(), other.clone()])
            .await
            .is_none()
    );
    let rows: i64 = Connection::open(fixture.database())
        .unwrap()
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();

    assert_eq!(rows, 1);
    assert_eq!(
        reopened.get(&other, &source, false).await.digest,
        Some(digest)
    );
    assert_eq!(reopened.get(&inactive, &source, false).await.digest, None);
}

#[tokio::test]
async fn every_binding_field_invalidates_old_fingerprints() {
    for changed in 0..3 {
        let fixture = Fixture::new();
        let cache = fixture.cache();
        fixture.seed(&cache, &fixture.selection).await;
        drop(cache);
        let mut binding = Binding::from(&fixture.pairing);
        match changed {
            0 => binding.server_url = "https://other-cache-test.example".into(),
            1 => binding.pc_id = crate::source::random_id().unwrap(),
            _ => binding.mirror_directory.push_str("-other"),
        }
        let mut database = CacheDatabase::open(
            &fixture.database(),
            &binding,
            std::slice::from_ref(&fixture.selection),
        )
        .unwrap();
        database
            .reconcile(&binding, std::slice::from_ref(&fixture.selection))
            .unwrap();
        drop(database);
        let rows: i64 = Connection::open(fixture.database())
            .unwrap()
            .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }
}
