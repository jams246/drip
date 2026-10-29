use super::*;

#[tokio::test]
async fn new_worker_status_does_not_restore_synced_roots_from_history() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.verification/persistent-cache/status-tests")
        .join(crate::source::random_id().unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    let (sender, _) = mpsc::channel(4);
    let (progress, _) = watch::channel(None);
    let previous = EventSink::new(sender, progress).with_log(&directory);
    let path = SourcePath::new('C', vec!["source".into()]).unwrap();
    previous
        .emit(
            EventKind::Synced,
            Some("selection"),
            Some(&path),
            "old",
            "done",
        )
        .await
        .unwrap();
    assert_eq!(
        crate::ActivityLog::new(&directory)
            .snapshot()
            .unwrap()
            .roots[0]
            .state,
        crate::engine::SelectionState::Synced
    );

    let (sender, _) = mpsc::channel(4);
    let (progress, _) = watch::channel(None);
    let current = EventSink::new(sender, progress).with_log(&directory);
    let status = current.subscribe_status();
    assert_eq!(
        status.borrow().connection,
        crate::engine::ConnectionState::Disconnected
    );
    assert!(status.borrow().roots.is_empty());
    current
        .emit(
            EventKind::Scanning,
            Some("selection"),
            Some(&path),
            "new",
            "checking current files",
        )
        .await
        .unwrap();
    assert_eq!(
        current.subscribe_status().borrow().roots[0].state,
        crate::engine::SelectionState::Scanning
    );
}
