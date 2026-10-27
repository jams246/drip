mod common;
mod mutation_helpers;

use std::fs;

use common::{Fixture, enroll, mirror_path, request, select_kind, upload};
use drip_core::SourcePath;
use drip_core::protocol::EntryKind;
use drip_server::StoreError;
use mutation_helpers::{child, create_directory, delete, entry};

#[tokio::test]
async fn deletion_checks_revision_and_removes_only_empty_directories() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Entry deletion");
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
    let blocked = child(&root, "blocked");
    create_directory(
        &fixture,
        credential,
        &selection_id,
        generation,
        blocked.clone(),
    );
    let file = child(&blocked, "child.txt");
    let file_path = mirror_path(&fixture, &device, &file);
    fs::write(&file_path, b"first").unwrap();
    let first = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let old_file = entry(&first.entries, &file);
    let blocked_entry = entry(&first.entries, &blocked);

    fs::write(&file_path, b"other").unwrap();
    assert!(matches!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &file,
            old_file
        ),
        Err(StoreError::Conflict)
    ));
    assert_eq!(fs::read(&file_path).unwrap(), b"other");

    let current = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let current_file = entry(&current.entries, &file);
    assert!(matches!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &blocked,
            blocked_entry,
        ),
        Err(StoreError::Conflict)
    ));
    assert!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &file,
            current_file,
        )
        .unwrap()
        .deleted
    );
    assert!(
        !delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &file,
            current_file,
        )
        .unwrap()
        .deleted
    );
    let replacement = b"replacement";
    upload(
        &fixture.store,
        credential,
        request(
            &selection_id,
            generation,
            file.clone(),
            replacement,
            drip_core::Timestamp::new(1_700_000_000, 123_456_789).unwrap(),
        ),
        replacement,
    )
    .await;
    assert!(matches!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &file,
            current_file,
        ),
        Err(StoreError::Conflict)
    ));
    assert_eq!(fs::read(&file_path).unwrap(), replacement);
    let current = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    assert!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &file,
            entry(&current.entries, &file),
        )
        .unwrap()
        .deleted
    );
    assert!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &blocked,
            blocked_entry,
        )
        .unwrap()
        .deleted
    );
    assert!(!mirror_path(&fixture, &device, &blocked).exists());
    assert!(mirror_path(&fixture, &device, &root).is_dir());
}

#[test]
fn stale_generation_cannot_delete_a_retained_mirror() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Stale deletion");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["retained".to_owned()]).unwrap();
    let selection_id = "f".repeat(32);
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
    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    fixture
        .store
        .update_selections(
            credential,
            drip_core::protocol::SelectionUpdate {
                expected_generation: generation,
                selections: Vec::new(),
            },
        )
        .unwrap();
    assert!(matches!(
        delete(
            &fixture,
            credential,
            &selection_id,
            generation,
            &root,
            entry(&page.entries, &root),
        ),
        Err(StoreError::Conflict)
    ));
    assert!(mirror_path(&fixture, &device, &root).is_dir());
}
