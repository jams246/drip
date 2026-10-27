use std::{
    fs::{self, OpenOptions},
    os::windows::fs::OpenOptionsExt,
};

use drip_core::protocol::EntryKind;

use super::{SourceError, TestDirectory};
use crate::{
    settings::{ClientSettings, SettingsError, SettingsStore},
    source::{capture_root, same_path},
};

#[test]
fn add_reactivates_retained_id_and_ancestor_reports_authority() {
    let directory = TestDirectory::new();
    let parent = directory.path.join("parent");
    let child = parent.join("child");
    fs::create_dir_all(&child).unwrap();
    let store = SettingsStore::open(&directory.path);
    let child_root = capture_root(&child).unwrap();
    let first = store.add(child_root.clone()).unwrap();
    assert!(store.remove(&first.selection.source).unwrap());
    let parent_result = store.add(capture_root(&parent).unwrap()).unwrap();

    assert_eq!(
        parent_result.authority_over,
        vec![first.selection.source.clone()]
    );
    assert!(matches!(
        store.add(capture_root(&child).unwrap()),
        Err(SettingsError::Selection(_))
    ));
    store.remove(&parent_result.selection.source).unwrap();
    let readded = store.add(capture_root(&child).unwrap()).unwrap();
    assert_eq!(readded.selection.id, first.selection.id);
}

#[test]
fn active_duplicate_is_rejected_and_volume_change_pauses() {
    let directory = TestDirectory::new();
    let root = directory.path.join("selected");
    fs::create_dir(&root).unwrap();
    let store = SettingsStore::open(&directory.path);
    let first = store.add(capture_root(&root).unwrap()).unwrap();
    assert!(matches!(
        store.add(capture_root(&root).unwrap()),
        Err(SettingsError::Selection(_))
    ));
    let mut tampered = first.selection;
    tampered.volume_identity.push_str("different");

    assert!(matches!(
        crate::source::validate_root(&tampered),
        Err(SourceError::VolumeChanged)
    ));
}

#[test]
fn concurrent_settings_updates_do_not_overwrite_each_other() {
    let directory = TestDirectory::new();
    let first_path = directory.path.join("first");
    let second_path = directory.path.join("second");
    fs::create_dir(&first_path).unwrap();
    fs::create_dir(&second_path).unwrap();
    let first_root = capture_root(&first_path).unwrap();
    let second_root = capture_root(&second_path).unwrap();
    let first_store = SettingsStore::open(&directory.path);
    let second_store = SettingsStore::open(&directory.path);

    let first = std::thread::spawn(move || first_store.add(first_root).unwrap());
    let second = std::thread::spawn(move || second_store.add(second_root).unwrap());
    first.join().unwrap();
    second.join().unwrap();

    assert_eq!(
        SettingsStore::open(&directory.path)
            .load()
            .unwrap()
            .selections
            .len(),
        2
    );
}

#[test]
fn acknowledging_old_update_rebases_newer_local_selections() {
    let directory = TestDirectory::new();
    let first_path = directory.path.join("first");
    let second_path = directory.path.join("second");
    fs::create_dir(&first_path).unwrap();
    fs::create_dir(&second_path).unwrap();
    let store = SettingsStore::open(&directory.path);
    store.add(capture_root(&first_path).unwrap()).unwrap();
    let sent = store.prepare_update(4).unwrap();
    store.add(capture_root(&second_path).unwrap()).unwrap();
    store.acknowledge_update(&sent, 5).unwrap();
    let current = store.load().unwrap();

    assert_eq!(current.acknowledged_generation, 5);
    let pending = current.pending_update.unwrap();
    assert_eq!(pending.expected_generation, 5);
    assert_eq!(pending.selections.len(), 2);
    assert_eq!(pending.selections[0].root_kind, EntryKind::Directory);
}

#[test]
fn prepare_update_recovers_a_lost_server_acknowledgement() {
    let directory = TestDirectory::new();
    let path = directory.path.join("selected");
    fs::create_dir(&path).unwrap();
    let store = SettingsStore::open(&directory.path);
    store.add(capture_root(&path).unwrap()).unwrap();
    let lost = store.prepare_update(0).unwrap();

    let recovered = store.prepare_update(1).unwrap();

    assert_eq!(lost.selections, recovered.selections);
    assert_eq!(recovered.expected_generation, 1);
    assert_eq!(store.load().unwrap().acknowledged_generation, 1);
}

#[test]
fn blocked_settings_replace_preserves_existing_bytes_and_cleans_temporary_file() {
    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    let original = ClientSettings {
        acknowledged_generation: 7,
        ..ClientSettings::default()
    };
    store.save(&original).unwrap();
    let path = directory.path.join("drip.json");
    let bytes = fs::read(&path).unwrap();
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&path)
        .unwrap();
    let replacement = ClientSettings {
        acknowledged_generation: 8,
        ..ClientSettings::default()
    };

    let SettingsError::Io { action, source, .. } = store.save(&replacement).unwrap_err() else {
        panic!("blocked replace should return a settings I/O error");
    };
    assert_eq!(action, "atomically replace");
    assert_eq!(
        source
            .get_ref()
            .and_then(|source| source.downcast_ref::<windows::core::Error>())
            .map(|source| source.code().0 as u32),
        Some(0x8007_0005)
    );
    drop(blocker);

    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(store.load().unwrap(), original);
    assert!(!fs::read_dir(&directory.path).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".drip.json.")
    }));
}

#[test]
fn blocked_settings_replace_retries_after_target_handle_closes() {
    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    store.save(&ClientSettings::default()).unwrap();
    let path = directory.path.join("drip.json");
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&path)
        .unwrap();
    let replacement = ClientSettings {
        acknowledged_generation: 8,
        ..ClientSettings::default()
    };
    let writer_store = SettingsStore::open(&directory.path);
    let (started, waiting) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        started.send(()).unwrap();
        writer_store.save(&replacement)
    });
    waiting.recv().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    drop(blocker);

    writer.join().unwrap().unwrap();
    assert_eq!(store.load().unwrap().acknowledged_generation, 8);
}

#[test]
fn root_kind_refresh_stages_new_generation_without_changing_volume() {
    let directory = TestDirectory::new();
    let path = directory.path.join("selected.txt");
    fs::write(&path, b"synthetic").unwrap();
    let store = SettingsStore::open(&directory.path);
    let added = store.add(capture_root(&path).unwrap()).unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    assert!(matches!(
        crate::source::validate_root(&added.selection),
        Err(crate::source::SourceError::RootKindChanged)
    ));

    let current = capture_root(&path).unwrap();
    assert_eq!(current.kind, EntryKind::Directory);
    assert!(same_path(&current.source, &added.selection.source));
    assert!(
        store
            .refresh_root_kind(&added.selection.id, current.kind)
            .unwrap()
    );
    let saved = store.load().unwrap();
    assert_eq!(
        saved.selections[0].volume_identity,
        added.selection.volume_identity
    );
    assert_eq!(
        saved.pending_update.unwrap().selections[0].root_kind,
        EntryKind::Directory
    );
}
