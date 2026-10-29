use rusqlite::Connection;

use super::support::Fixture;
use crate::engine::watcher::DirtySignal;

#[tokio::test]
async fn conflicting_change_epoch_rolls_back_cache_pruning() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    fixture.seed(&cache, &fixture.selection).await;
    let signal = DirtySignal::default();
    let epoch = signal.epoch(&fixture.selection.id);
    signal.mark(&fixture.selection.id, false);

    assert!(
        cache
            .prune(&fixture.selection.id, &[], signal, epoch)
            .await
            .is_none()
    );
    let rows: i64 = Connection::open(fixture.database())
        .unwrap()
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}
