use std::fs;

use super::TestDirectory;
use crate::{settings::SettingsStore, source::capture_root};

#[test]
fn canonical_root_and_ancestor_case_refresh_preserves_identity_and_volume() {
    let directory = TestDirectory::new();
    let parent = directory.path.join("parent");
    let selected = parent.join("é-root");
    fs::create_dir_all(&selected).unwrap();
    let store = SettingsStore::open(&directory.path);
    let added = store.add(capture_root(&selected).unwrap()).unwrap();
    fs::rename(&selected, parent.join("É-ROOT")).unwrap();
    fs::rename(&parent, directory.path.join("PARENT")).unwrap();
    let current = capture_root(&directory.path.join("PARENT/É-ROOT")).unwrap();
    assert_ne!(current.source, added.selection.source);
    assert!(store.refresh_root(&added.selection.id, &current).unwrap());
    let settings = store.load().unwrap();
    let refreshed = &settings.selections[0];
    assert_eq!(refreshed.id, added.selection.id);
    assert_eq!(refreshed.volume_identity, added.selection.volume_identity);
    assert_eq!(refreshed.source, current.source);
    assert_eq!(
        settings.pending_update.unwrap().selections[0].source,
        current.source
    );
}

#[test]
fn canonical_refresh_rejects_another_path_without_changing_saved_state() {
    let directory = TestDirectory::new();
    let selected = directory.path.join("selected");
    let other = directory.path.join("different");
    fs::create_dir(&selected).unwrap();
    fs::create_dir(&other).unwrap();
    let store = SettingsStore::open(&directory.path);
    let added = store.add(capture_root(&selected).unwrap()).unwrap();
    let before = store.load().unwrap();
    assert!(
        store
            .refresh_root(&added.selection.id, &capture_root(&other).unwrap())
            .is_err()
    );
    assert_eq!(store.load().unwrap(), before);
}
