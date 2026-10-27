mod common;

use std::{collections::HashSet, fs};

use common::{Fixture, enroll, mirror_path, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::{DirectoryCreateRequest, EntryKind};
use drip_server::StoreError;

#[test]
fn long_path_inventory_pages_stay_bounded_and_detect_missing_anchors() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Long inventory path");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["tree".to_owned()]).unwrap();
    let selection_id = "e".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::Directory,
    );
    create_directory(
        &fixture,
        credential,
        &selection_id,
        generation,
        root.clone(),
    );

    let mut expected = HashSet::from([root.clone()]);
    let mut parent = root;
    let mut parent_directory = mirror_path(&fixture, &device, &parent);
    for index in 0..17 {
        let name = format!("segment-{index:02}-{}", "x".repeat(190));
        parent = child(&parent, &name);
        parent_directory.push(&name);
        create_directory(
            &fixture,
            credential,
            &selection_id,
            generation,
            parent.clone(),
        );
        expected.insert(parent.clone());
    }

    for index in 0..1100 {
        let name = format!("file-{index:04}");
        fs::write(parent_directory.join(&name), b"").unwrap();
        expected.insert(child(&parent, &name));
    }
    assert!(
        serde_json::to_vec(&child(&parent, "file-0000"))
            .unwrap()
            .len()
            > 2048
    );

    let mut seen = HashSet::new();
    let mut cursor = None;
    let mut first_cursor = None;
    let mut first_anchor = None;
    let mut pages = 0;
    loop {
        let page = fixture
            .store
            .list_entries_at_generation(
                credential,
                &selection_id,
                Some(generation),
                cursor.as_deref(),
            )
            .unwrap();
        assert!(!page.entries.is_empty());
        assert!(page.entries.len() <= 1024);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 3 * 1024 * 1024);
        let next = page.next_cursor;
        if first_cursor.is_none()
            && let Some(next) = &next
        {
            assert!(next.starts_with("b3-"));
            assert!(page.entries.len() < 1024);
            assert!(
                next.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            );
            first_cursor = Some(next.clone());
            first_anchor = page.entries.last().map(|entry| entry.source_path.clone());
        }
        for entry in page.entries {
            assert!(seen.insert(entry.source_path));
        }
        pages += 1;
        let Some(next) = next else {
            break;
        };
        assert!(next.len() <= 4096);
        cursor = Some(next);
    }
    assert!(pages > 1);
    assert_eq!(seen, expected);

    let first_cursor = first_cursor.unwrap();
    let anchor = first_anchor.unwrap();
    fs::remove_file(mirror_path(&fixture, &device, &anchor)).unwrap();
    assert!(matches!(
        fixture.store.list_entries_at_generation(
            credential,
            &selection_id,
            Some(generation),
            Some(&first_cursor),
        ),
        Err(StoreError::Conflict)
    ));
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

fn child(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
