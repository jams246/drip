use rusqlite::Connection;

use super::support::Fixture;
use crate::source::SourceFile;

#[tokio::test]
async fn reopened_store_reuses_digest_without_rewriting_verification_time() {
    let fixture = Fixture::new();
    let first = fixture.cache();
    let digest = fixture.seed(&first, &fixture.selection).await;
    drop(first);

    let before = stored_record(&fixture.database());
    let modified = std::fs::metadata(fixture.database())
        .unwrap()
        .modified()
        .unwrap();
    let reopened = fixture.cache();
    assert!(
        reopened
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    let source = SourceFile::open(&fixture.file).unwrap();
    source.verify().await.unwrap();
    let lookup = reopened.get(&fixture.selection, &source, false).await;

    assert_eq!(lookup.digest, Some(digest));
    drop(reopened);
    assert_eq!(stored_record(&fixture.database()), before);
    assert_eq!(
        std::fs::metadata(fixture.database())
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
}

fn stored_record(path: &std::path::Path) -> (Vec<u8>, i64, i64, i64) {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT digest, verified_seconds, verified_nanos, count(*) OVER () FROM cached_files",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}
