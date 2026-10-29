use std::{fs, os::windows::fs::OpenOptionsExt, path::Path};

use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::{SourceError, TestDirectory};
use crate::{
    settings::SavedSelection,
    source::{capture_root, confirm_root_absent},
};

fn saved(path: &Path) -> SavedSelection {
    let root = capture_root(path).unwrap();
    SavedSelection {
        id: "deleted-root".into(),
        source: root.source,
        root_kind: root.kind,
        volume_identity: root.volume_identity,
        active: true,
    }
}

#[test]
fn deleted_file_is_absent_but_recreated_file_is_not() {
    let fixture = TestDirectory::new();
    let path = fixture.path.join("source.txt");
    fs::write(&path, b"synthetic").unwrap();
    let selection = saved(&path);
    assert!(!confirm_root_absent(&selection).unwrap());
    fs::remove_file(&path).unwrap();
    assert!(confirm_root_absent(&selection).unwrap());
    fs::write(&path, b"recreated").unwrap();
    assert!(!confirm_root_absent(&selection).unwrap());
}

#[test]
fn deleted_folder_or_ancestor_is_confirmed_on_the_same_volume() {
    let fixture = TestDirectory::new();
    let parent = fixture.path.join("parent");
    let root = parent.join("selected");
    fs::create_dir_all(root.join("child")).unwrap();
    fs::write(root.join("child/file.txt"), b"synthetic").unwrap();
    let selection = saved(&root);
    fs::remove_dir_all(&root).unwrap();
    assert!(confirm_root_absent(&selection).unwrap());
    fs::remove_dir(&parent).unwrap();
    assert!(confirm_root_absent(&selection).unwrap());
}

#[test]
fn changed_volume_or_inaccessible_parent_cannot_authorize_deletion() {
    let fixture = TestDirectory::new();
    let path = fixture.path.join("selected.txt");
    fs::write(&path, b"synthetic").unwrap();
    let selection = saved(&path);
    fs::remove_file(&path).unwrap();
    let mut changed = selection.clone();
    changed.volume_identity.push('x');
    assert!(matches!(
        confirm_root_absent(&changed),
        Err(SourceError::VolumeChanged)
    ));
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(&fixture.path)
        .unwrap();
    assert!(confirm_root_absent(&selection).is_err());
    drop(locked);
    assert!(confirm_root_absent(&selection).unwrap());
}

#[test]
fn file_replacing_an_ancestor_does_not_authorize_deletion() {
    let fixture = TestDirectory::new();
    let parent = fixture.path.join("parent");
    let root = parent.join("selected");
    fs::create_dir_all(&root).unwrap();
    let selection = saved(&root);
    fs::remove_dir_all(&parent).unwrap();
    fs::write(&parent, b"replacement").unwrap();
    assert!(confirm_root_absent(&selection).is_err());
}

#[test]
fn watch_retirement_requires_current_settings_and_reconfirmed_absence() {
    let fixture = TestDirectory::new();
    let app = fixture.path.join("app");
    fs::create_dir(&app).unwrap();
    let path = fixture.path.join("selected.txt");
    fs::write(&path, b"synthetic").unwrap();
    let store = crate::settings::SettingsStore::open(&app);
    let selection = store.add(capture_root(&path).unwrap()).unwrap().selection;
    let pending = store.load().unwrap().pending_update.unwrap();
    assert!(!store.remove_deleted_root(&selection, 0).unwrap());
    store.acknowledge_update(&pending, 1).unwrap();
    assert!(!store.remove_deleted_root(&selection, 1).unwrap());
    fs::remove_file(&path).unwrap();
    assert!(!store.remove_deleted_root(&selection, 2).unwrap());
    assert!(store.remove_deleted_root(&selection, 1).unwrap());
    let settings = store.load().unwrap();
    assert!(!settings.selections[0].active);
    assert!(!settings.pending_update.unwrap().selections[0].active);
    fs::write(&path, b"recreated").unwrap();
    let reactivated = store.add(capture_root(&path).unwrap()).unwrap();
    assert_eq!(reactivated.selection.id, selection.id);
    assert!(reactivated.selection.active);
}

#[test]
fn reparse_point_replacing_a_deleted_root_cannot_authorize_deletion() {
    let fixture = TestDirectory::new();
    let root = fixture.path.join("selected");
    let outside = fixture.path.join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    let selection = saved(&root);
    fs::remove_dir(&root).unwrap();
    let command = format!(
        "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        root.display(),
        outside.display()
    );
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(matches!(
        confirm_root_absent(&selection),
        Err(SourceError::ReparsePoint(_))
    ));
    fs::remove_dir(&root).unwrap();
}
