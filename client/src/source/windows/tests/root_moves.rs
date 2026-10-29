use std::fs;

use super::TestDirectory;
use crate::{
    settings::SettingsStore,
    source::{capture_root, resolve_root},
};

#[test]
fn file_identity_follows_a_rename_and_move_outside_the_original_parent() {
    let fixture = TestDirectory::new();
    let old = fixture.path.join("selected.txt");
    let destination = fixture.path.join("unwatched/nested/renamed.txt");
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&old, b"same identity").unwrap();
    let store = SettingsStore::open(&fixture.path);
    let saved = store.add(capture_root(&old).unwrap()).unwrap().selection;
    fs::rename(&old, &destination).unwrap();
    let moved = resolve_root(&saved).unwrap().unwrap();
    assert_eq!(moved.source, capture_root(&destination).unwrap().source);
    assert_eq!(Some(moved.identity), saved.root_identity);
    assert!(store.follow_root(&saved, &moved).unwrap());
    let current = store.load().unwrap();
    assert_eq!(current.selections[0].id, saved.id);
    assert!(current.selections[0].active);
    assert_eq!(current.selections[0].source, moved.source);
    assert_eq!(
        current.pending_update.unwrap().selections[0].source,
        moved.source
    );
}

#[test]
fn persisted_identity_follows_a_moved_ancestor_after_settings_reload() {
    let fixture = TestDirectory::new();
    let parent = fixture.path.join("parent");
    let selected = parent.join("selected");
    fs::create_dir_all(&selected).unwrap();
    let store = SettingsStore::open(&fixture.path);
    store.add(capture_root(&selected).unwrap()).unwrap();
    fs::rename(&parent, fixture.path.join("moved-parent")).unwrap();
    let saved = store.load().unwrap().selections.remove(0);
    let moved = resolve_root(&saved).unwrap().unwrap();
    assert_eq!(
        moved.source,
        capture_root(&fixture.path.join("moved-parent/selected"))
            .unwrap()
            .source
    );
    assert_eq!(Some(moved.identity), saved.root_identity);
    assert!(!crate::source::confirm_root_absent(&saved).unwrap());
}

#[test]
fn old_path_replacement_does_not_steal_a_moved_watch() {
    let fixture = TestDirectory::new();
    let old = fixture.path.join("source.txt");
    let new = fixture.path.join("moved.txt");
    fs::write(&old, b"original").unwrap();
    let store = SettingsStore::open(&fixture.path);
    let saved = store.add(capture_root(&old).unwrap()).unwrap().selection;
    fs::rename(&old, &new).unwrap();
    fs::write(&old, b"replacement").unwrap();
    assert_eq!(
        resolve_root(&saved).unwrap().unwrap().source,
        capture_root(&new).unwrap().source
    );
    fs::remove_file(&new).unwrap();
    assert_eq!(resolve_root(&saved).unwrap().unwrap().source, saved.source);
}
