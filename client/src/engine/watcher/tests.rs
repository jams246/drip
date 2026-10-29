use super::*;
use notify::{
    Event, EventKind,
    event::{AccessKind, CreateKind, ModifyKind, RemoveKind, RenameMode},
};

fn roots() -> Vec<SavedSelection> {
    [("a", r"C:\sources\folder"), ("b", r"C:\sources\test.txt")]
        .into_iter()
        .map(|(id, path)| SavedSelection {
            id: id.into(),
            source: drip_core::SourcePath::parse_absolute(path).unwrap(),
            root_kind: drip_core::protocol::EntryKind::Directory,
            volume_identity: "volume".into(),
            root_identity: None,
            active: true,
        })
        .collect()
}

fn event(signal: &DirtySignal, kind: EventKind, paths: &[&str]) {
    let mut event = Event::new(kind);
    event.paths = paths.iter().map(PathBuf::from).collect();
    routing::route(signal, &roots(), Path::new(r"\\?\C:\app"), Ok(event));
}

#[test]
fn file_edits_and_replacement_saves_target_only_the_selected_root() {
    let signal = DirtySignal::default();
    for kind in [
        EventKind::Modify(ModifyKind::Any),
        EventKind::Create(CreateKind::File),
    ] {
        event(&signal, kind, &[r"C:\sources\test.txt"]);
        assert_eq!(signal.take().selections, HashSet::from(["b".into()]));
    }
    event(
        &signal,
        EventKind::Modify(ModifyKind::Any),
        &[r"\\?\c:\SOURCES\FOLDER\nested.txt"],
    );
    assert_eq!(signal.take().selections, HashSet::from(["a".into()]));
}

#[test]
fn siblings_parent_metadata_access_and_internal_files_do_not_queue_work() {
    let signal = DirtySignal::default();
    event(
        &signal,
        EventKind::Modify(ModifyKind::Any),
        &[r"C:\sources", r"C:\sources\folder-extra\file.txt"],
    );
    event(
        &signal,
        EventKind::Create(CreateKind::File),
        &[r"C:\sources\other.txt"],
    );
    event(
        &signal,
        EventKind::Access(AccessKind::Read),
        &[r"C:\sources\test.txt"],
    );
    event(
        &signal,
        EventKind::Modify(ModifyKind::Any),
        &[
            r"C:\app\.drip.json.42.tmp",
            r"C:\app\drip.log.1",
            r"C:\app\drip.state.sqlite3",
            r"C:\app\drip.state.sqlite3-journal",
            r"C:\app\drip.state.sqlite3-wal",
            r"C:\app\drip.state.sqlite3-shm",
            r"C:\app\drip.state.corrupt.id.sqlite3",
        ],
    );
    assert!(!signal.pending());
}

#[test]
fn renames_consider_both_endpoints_and_root_recreation_is_uncertain() {
    let signal = DirtySignal::default();
    event(
        &signal,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        &[r"C:\sources\folder\file.txt", r"C:\sources\test.txt"],
    );
    assert_eq!(
        signal.take().selections,
        HashSet::from(["a".into(), "b".into()])
    );
    for kind in [
        EventKind::Remove(RemoveKind::Folder),
        EventKind::Create(CreateKind::Folder),
    ] {
        event(&signal, kind, &[r"C:\sources\folder"]);
        assert_eq!(signal.take().uncertain, HashSet::from(["a".into()]));
    }
}

#[test]
fn settings_refresh_invalidates_authority_without_queuing_all_roots() {
    let signal = DirtySignal::default();
    let epoch = signal.epoch("a");
    event(
        &signal,
        EventKind::Create(CreateKind::File),
        &[r"C:\app\DRIP.JSON"],
    );
    assert!(signal.pending());
    assert!(signal.changed_since("a", epoch));
    let batch = signal.take();
    assert!(batch.selections.is_empty() && !batch.full);
}

#[test]
fn watcher_errors_preserve_known_scope_and_unknown_errors_force_all() {
    let signal = DirtySignal::default();
    routing::route(
        &signal,
        &roots(),
        Path::new(r"C:\app"),
        Err(notify::Error::generic("test").add_path(PathBuf::from(r"C:\sources\test.txt"))),
    );
    let batch = signal.take();
    assert!(!batch.full && batch.uncertain.contains("b") && !batch.uncertain.contains("a"));
    routing::route(
        &signal,
        &roots(),
        Path::new(r"C:\app"),
        Err(notify::Error::generic("test")),
    );
    assert!(signal.take().full);
}

#[test]
fn recursive_registration_upgrades_a_previously_watched_parent() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/watch-isolation/watcher-tests")
        .join(crate::source::random_id().unwrap());
    std::fs::create_dir_all(root.join("nested")).unwrap();
    let mut watcher = SourceWatcher::new(DirtySignal::default(), root.clone(), Vec::new()).unwrap();
    watcher.watch(&root.join("nested"), true).unwrap();
    watcher.watch(&root, true).unwrap();
    assert!(
        watcher
            .watched
            .iter()
            .any(|(path, mode)| path == &root && *mode == RecursiveMode::Recursive)
    );
    drop(watcher);
    std::fs::remove_dir_all(root).unwrap();
}
