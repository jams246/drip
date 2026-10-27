use std::{fs, path::Path, process::Command};

use drip_core::{SourcePath, protocol::EntryKind};

use super::super::{SourceError, SourceRoot, capture_root, confirm_absent, scan_root};
use super::TestDirectory;
use crate::settings::SavedSelection;

#[test]
fn recursive_scan_keeps_unicode_empty_entries_and_excludes_app_state() {
    let directory = TestDirectory::new();
    let root = directory.path.join("selected");
    let app = root.join("app");
    fs::create_dir_all(app.join("empty")).unwrap();
    fs::write(root.join("雪🙂.txt"), b"synthetic").unwrap();
    fs::write(root.join("empty.bin"), []).unwrap();
    fs::write(app.join("drip.json"), b"settings").unwrap();
    fs::write(app.join(".drip.json.1.0.tmp"), b"replacement").unwrap();
    fs::write(app.join("drip.settings.lock"), []).unwrap();
    fs::write(app.join("drip.run.lock"), []).unwrap();
    let saved = saved(&root);

    let scan = scan_root(&saved, &app).unwrap();

    assert!(scan.complete, "{:?}", scan.issues);
    assert_eq!(scan.root_kind, EntryKind::Directory);
    assert!(scan.entries.iter().any(|entry| {
        entry
            .source_path
            .components()
            .last()
            .is_some_and(|name| name == "雪🙂.txt")
    }));
    assert!(scan.entries.iter().any(|entry| {
        entry
            .source_path
            .components()
            .last()
            .is_some_and(|name| name == "empty.bin")
            && entry.size == 0
    }));
    assert!(scan.entries.iter().any(|entry| {
        entry
            .source_path
            .components()
            .last()
            .is_some_and(|name| name == "empty")
            && entry.kind == EntryKind::Directory
    }));
    assert_eq!(scan.protected_paths.len(), 4);
}

#[test]
fn recursive_scan_skips_junction_and_removes_delete_authority() {
    let directory = TestDirectory::new();
    let root = directory.path.join("selected");
    let app = directory.path.join("app");
    let outside = directory.path.join("outside");
    let junction = root.join("redirect");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("good.txt"), b"synthetic").unwrap();
    fs::write(outside.join("secret.bin"), b"synthetic").unwrap();
    let command = format!(
        "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        junction.display(),
        outside.display()
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved = saved(&root);

    let scan = scan_root(&saved, &app).unwrap();

    assert!(!scan.complete);
    assert!(scan.entries.iter().any(|entry| {
        entry
            .source_path
            .components()
            .last()
            .is_some_and(|name| name == "good.txt")
    }));
    assert!(
        scan.issues
            .iter()
            .any(|issue| issue.source_path.as_ref().is_some_and(|path| path
                .components()
                .last()
                .is_some_and(|name| name == "redirect")))
    );
    assert!(!scan.entries.iter().any(|entry| {
        entry
            .source_path
            .components()
            .last()
            .is_some_and(|name| name == "secret.bin")
    }));
}

#[test]
fn deletion_recheck_uses_exact_component_spelling() {
    let directory = TestDirectory::new();
    let root = directory.path.join("selected");
    let app = directory.path.join("app");
    fs::create_dir_all(root.join("NewCase")).unwrap();
    fs::create_dir_all(&app).unwrap();
    let saved = saved(&root);
    let old_spelling = child(&saved.source, "newcase");
    let actual_spelling = child(&saved.source, "NewCase");

    assert!(confirm_absent(&saved, &old_spelling, &app).unwrap());
    assert!(!confirm_absent(&saved, &actual_spelling, &app).unwrap());
}

#[test]
fn deleted_selected_file_is_unavailable_not_absent() {
    let directory = TestDirectory::new();
    let path = directory.path.join("selected.txt");
    fs::write(&path, b"synthetic").unwrap();
    let saved = saved(&path);
    fs::remove_file(path).unwrap();

    assert!(matches!(
        crate::source::validate_root(&saved),
        Err(SourceError::RootUnavailable(_))
    ));
}

#[test]
fn stale_root_spelling_is_absent_only_under_the_current_canonical_selection() {
    let directory = TestDirectory::new();
    let root = directory.path.join("canonical");
    let app = directory.path.join("app");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&app).unwrap();
    fs::write(root.join("visible.bin"), b"synthetic").unwrap();
    let saved = saved(&root);
    let mut components = saved.source.components().to_vec();
    *components.last_mut().unwrap() = "CANONICAL".to_owned();
    let old_root = SourcePath::new(saved.source.drive(), components).unwrap();

    assert!(confirm_absent(&saved, &old_root, &app).unwrap());
    assert!(confirm_absent(&saved, &child(&old_root, "visible.bin"), &app).unwrap());
    assert!(!confirm_absent(&saved, &saved.source, &app).unwrap());
    assert!(!confirm_absent(&saved, &child(&saved.source, "visible.bin"), &app).unwrap());

    fs::rename(&root, directory.path.join("CANONICAL")).unwrap();
    assert!(matches!(
        confirm_absent(&saved, &child(&saved.source, "missing.bin"), &app),
        Err(SourceError::Changed)
    ));
}

#[test]
fn selected_file_spelling_alias_is_absent_but_current_file_is_retained() {
    let directory = TestDirectory::new();
    let file = directory.path.join("canonical.bin");
    fs::write(&file, b"synthetic").unwrap();
    let saved = saved(&file);
    let mut components = saved.source.components().to_vec();
    *components.last_mut().unwrap() = "CANONICAL.bin".to_owned();
    let old = SourcePath::new(saved.source.drive(), components).unwrap();
    assert!(confirm_absent(&saved, &old, &directory.path).unwrap());
    assert!(!confirm_absent(&saved, &saved.source, &directory.path).unwrap());
}

fn saved(path: &Path) -> SavedSelection {
    let root: SourceRoot = capture_root(path).unwrap();
    SavedSelection {
        id: "selection".to_owned(),
        source: root.source,
        root_kind: root.kind,
        volume_identity: root.volume_identity,
        active: true,
    }
}

fn child(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
