use super::*;

#[test]
fn state_files_and_subtrees_are_ignored_under_application_and_parent_selections() {
    let signal = DirtySignal::default();
    let mut selections = roots();
    selections[0].source = drip_core::SourcePath::parse_absolute(r"C:\app").unwrap();
    selections[1].source = drip_core::SourcePath::parse_absolute(r"C:\").unwrap();
    for path in [
        r"C:\app\drip.state.sqlite3",
        r"C:\app\drip.state.sqlite3-journal",
        r"C:\app\drip.state.sqlite3-wal",
        r"C:\app\drip.state.sqlite3-shm",
        r"C:\app\drip.state.corrupt.id.sqlite3-journal",
        r"C:\app\drip.state.backup.folder\private.bin",
        r"C:\APP\DRIP.STATE.corrupt.set\nested\private.bin",
    ] {
        let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
        event.paths.push(PathBuf::from(path));
        routing::route(&signal, &selections, Path::new(r"C:\app"), Ok(event));
        assert!(
            !signal.pending(),
            "state path queued synchronization: {path}"
        );
    }
    let mut user_event = Event::new(EventKind::Modify(ModifyKind::Any));
    user_event
        .paths
        .push(PathBuf::from(r"C:\app\drip.stateful.user.txt"));
    routing::route(&signal, &selections, Path::new(r"C:\app"), Ok(user_event));
    assert_eq!(signal.take().selections.len(), 2);
}

#[tokio::test]
async fn native_state_writes_do_not_feed_back_under_application_or_parent_selection() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/persistent-cache/watcher-exclusions");
    for select_parent in [false, true] {
        let directory = base.join(crate::source::random_id().unwrap());
        let app = directory.join("app");
        std::fs::create_dir_all(&app).unwrap();
        let selected = if select_parent { &directory } else { &app };
        let captured = crate::source::capture_root(selected).unwrap();
        let saved = SavedSelection {
            id: "state-exclusion".into(),
            source: captured.source,
            root_kind: captured.kind,
            volume_identity: captured.volume_identity,
            root_identity: None,
            active: true,
        };
        let signal = DirtySignal::default();
        let mut watcher = SourceWatcher::new(signal.clone(), app.clone(), vec![saved]).unwrap();
        watcher.watch(selected, true).unwrap();
        watcher.watch_settings(&app).unwrap();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured_events = observed.clone();
        let mut probe = notify::recommended_watcher(move |event: notify::Result<Event>| {
            captured_events.lock().unwrap().push(event);
        })
        .unwrap();
        for (path, mode) in &watcher.watched {
            probe.watch(path, *mode).unwrap();
        }
        for name in [
            "drip.state.sqlite3",
            "drip.state.sqlite3-journal",
            "drip.state.corrupt.id.sqlite3",
        ] {
            std::fs::write(app.join(name), b"owned state").unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(
            !signal.pending(),
            "state writes caused watcher feedback; parent={select_parent}; events={:?}",
            observed.lock().unwrap()
        );
        drop(probe);
        drop(watcher);
        assert!(directory.is_absolute() && directory.starts_with(&base));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
