mod common;
#[cfg(windows)]
#[path = "inventory_case_aliases/windows.rs"]
mod windows;

#[cfg(not(windows))]
use std::collections::HashSet;
use std::fs;

use common::{Fixture, enroll, mirror_path, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::{DirectoryCreateRequest, EntryKind};

#[test]
#[cfg(not(windows))]
fn inventory_finds_case_alias_roots_and_paginates_only_selected_scope() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Case alias inventory");
    let credential = device.device_credential.expose_secret();
    let child_root = source(&["Workspace", "CaseAuthority", "child"]);
    let child_id = "c".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &child_id,
        child_root.clone(),
        EntryKind::Directory,
    );
    create_directory(
        &fixture,
        credential,
        &child_id,
        generation,
        child_root.clone(),
    );

    let sibling_root = source(&["workspace", "caseauthority-sibling"]);
    let sibling_id = "b".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &sibling_id,
        sibling_root.clone(),
        EntryKind::Directory,
    );
    create_directory(
        &fixture,
        credential,
        &sibling_id,
        generation,
        sibling_root.clone(),
    );
    fs::write(
        mirror_path(&fixture, &device, &sibling_root).join("outside.bin"),
        b"sibling",
    )
    .unwrap();

    let active_root = source(&["workspace", "caseauthority"]);
    let active_id = "a".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &active_id,
        active_root.clone(),
        EntryKind::Directory,
    );
    let old_root = source(&["Workspace", "CaseAuthority"]);
    let old_child_file = child(&child_root, "retained.bin");
    fs::write(
        mirror_path(&fixture, &device, &old_child_file),
        b"retained under inactive child",
    )
    .unwrap();
    let old_directory = mirror_path(&fixture, &device, &old_root);
    for index in 0..1021 {
        fs::write(old_directory.join(format!("old-{index:04}.bin")), b"old").unwrap();
    }
    let outside_parent_alias = old_directory
        .parent()
        .unwrap()
        .join("caseauthority-sibling");
    fs::create_dir_all(&outside_parent_alias).unwrap();
    fs::write(outside_parent_alias.join("unselected.bin"), b"sibling").unwrap();

    let before_canonical_root = fixture
        .store
        .list_entries_at_generation(credential, &active_id, Some(generation), None)
        .unwrap();
    assert_eq!(before_canonical_root.entries.len(), 1024);
    assert!(before_canonical_root.next_cursor.is_none());
    assert!(
        before_canonical_root
            .entries
            .iter()
            .any(|entry| entry.source_path == old_root)
    );
    assert!(
        before_canonical_root
            .entries
            .iter()
            .any(|entry| entry.source_path == old_child_file)
    );
    assert!(!mirror_path(&fixture, &device, &active_root).exists());

    create_directory(
        &fixture,
        credential,
        &active_id,
        generation,
        active_root.clone(),
    );
    for index in 0..4 {
        fs::write(
            mirror_path(&fixture, &device, &active_root).join(format!("new-{index:02}.bin")),
            b"new",
        )
        .unwrap();
    }

    let other_device = enroll(&fixture.store, "Other PC");
    let other_credential = other_device.device_credential.expose_secret();
    let other_generation = select_kind(
        &fixture.store,
        other_credential,
        &active_id,
        active_root.clone(),
        EntryKind::Directory,
    );
    create_directory(
        &fixture,
        other_credential,
        &active_id,
        other_generation,
        active_root.clone(),
    );
    fs::write(
        mirror_path(&fixture, &other_device, &active_root).join("foreign.bin"),
        b"other PC",
    )
    .unwrap();

    let mut expected = HashSet::from([old_root.clone(), child(&old_root, "child"), old_child_file]);
    expected.extend((0..1021).map(|index| child(&old_root, &format!("old-{index:04}.bin"))));
    expected.insert(active_root.clone());
    expected.extend((0..4).map(|index| child(&active_root, &format!("new-{index:02}.bin"))));
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = fixture
            .store
            .list_entries_at_generation(credential, &active_id, Some(generation), cursor.as_deref())
            .unwrap();
        assert!(page.entries.len() <= 1024);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 3 * 1024 * 1024);
        for entry in page.entries {
            assert!(seen.insert(entry.source_path.clone()));
            ordered.push(entry.source_path);
        }
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(pages, 2);
    assert_eq!(seen, expected);
    assert!(
        ordered
            .windows(2)
            .all(|pair| pair[0].components() < pair[1].components())
    );
    assert!(!seen.contains(&sibling_root));
    assert!(!seen.contains(&source(&["Workspace", "caseauthority-sibling"])));
    assert!(!seen.contains(&child(&active_root, "foreign.bin")));
}

fn create_directory(
    fixture: &Fixture,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source_path: SourcePath,
) {
    fixture
        .store
        .create_directory(
            credential,
            DirectoryCreateRequest {
                selection_id: selection_id.to_owned(),
                generation,
                source_path,
                expected_entry: None,
            },
        )
        .unwrap();
}

fn source(components: &[&str]) -> SourcePath {
    SourcePath::new(
        'C',
        components
            .iter()
            .map(|component| (*component).to_owned())
            .collect(),
    )
    .unwrap()
}

#[cfg(not(windows))]
fn child(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
