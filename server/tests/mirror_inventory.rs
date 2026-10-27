mod common;

use std::fs;

use common::{Fixture, enroll, mirror_path, request, select_kind, upload};
use drip_core::protocol::{DirectoryCreateRequest, EntryKind};
use drip_core::{SourcePath, Timestamp};
use drip_server::StoreError;

#[tokio::test]
async fn recursive_selection_uploads_descendants_only() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Recursive upload");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["folder".to_owned()]).unwrap();
    let selection_id = "c".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::Directory,
    );
    let source = child(&child(&root, "nested"), "file.txt");
    let bytes = b"nested upload";
    let (_, commit) = upload(
        &fixture.store,
        credential,
        request(
            &selection_id,
            generation,
            source.clone(),
            bytes,
            timestamp(),
        ),
        bytes,
    )
    .await;
    assert_eq!(commit.entry.source_path, source);
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
        bytes
    );

    let outside = SourcePath::new('C', vec!["outside.txt".to_owned()]).unwrap();
    assert!(matches!(
        fixture.store.prepare_upload(
            credential,
            request(&selection_id, generation, outside, bytes, timestamp()),
        ),
        Err(StoreError::DisallowedNamespace)
    ));
}

#[test]
fn recursive_inventory_returns_actual_metadata_in_pages_of_1024() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Recursive inventory");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["tree".to_owned()]).unwrap();
    let selection_id = "d".repeat(32);
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
    create_directory(
        &fixture,
        credential,
        &selection_id,
        generation,
        child(&root, "empty"),
    );
    let nested = child(&root, "nested");
    create_directory(
        &fixture,
        credential,
        &selection_id,
        generation,
        nested.clone(),
    );

    let tree = mirror_path(&fixture, &device, &root);
    fs::write(tree.join("nested/inside.bin"), b"nested actual bytes").unwrap();
    for index in 0..1020 {
        fs::write(tree.join(format!("entry-{index:04}")), b"").unwrap();
    }
    fs::write(tree.join("é.txt"), b"unicode").unwrap();

    let first = fixture
        .store
        .list_entries_at_generation(credential, &selection_id, Some(generation), None)
        .unwrap();
    assert_eq!(first.entries.len(), 1024);
    assert_eq!(first.entries[0].source_path, root);
    assert_eq!(first.entries[0].kind, EntryKind::Directory);
    assert!(first.next_cursor.is_some());
    assert!(first.entries.iter().any(|entry| {
        entry.source_path == child(&root, "empty") && entry.kind == EntryKind::Directory
    }));
    assert!(first.entries.iter().any(|entry| {
        entry.source_path == child(&nested, "inside.bin")
            && entry.digest
                == Some(drip_core::Blake3Digest::new(
                    *blake3::hash(b"nested actual bytes").as_bytes(),
                ))
    }));

    let second = fixture
        .store
        .list_entries_at_generation(
            credential,
            &selection_id,
            Some(generation),
            first.next_cursor.as_deref(),
        )
        .unwrap();
    assert_eq!(second.entries.len(), 1);
    assert_eq!(second.entries[0].source_path, child(&root, "é.txt"));
    assert!(second.next_cursor.is_none());
    assert!(matches!(
        fixture.store.list_entries_at_generation(
            credential,
            &selection_id,
            Some(generation - 1),
            None,
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

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}
