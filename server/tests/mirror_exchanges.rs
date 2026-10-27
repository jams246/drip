mod common;
#[path = "mirror_exchanges/helpers.rs"]
mod helpers;

use std::fs;

use axum::body::Body;
use common::{Fixture, enroll, mirror_path, request, select_kind, upload};
use drip_core::protocol::{DirectoryCreateRequest, EntryKind};
use drip_core::{SourcePath, Timestamp};
use drip_server::StoreError;
use helpers::{child, entry_precondition, selection};

#[tokio::test]
async fn upload_exchanges_an_empty_directory_for_a_verified_file() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Directory to file");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["tree".to_owned()]).unwrap();
    let selection_id = "a".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::Directory,
    );
    let target = child(&root, "branch");
    fixture
        .store
        .create_directory(
            credential,
            DirectoryCreateRequest {
                selection_id: selection_id.clone(),
                generation,
                source_path: target.clone(),
                expected_entry: None,
            },
        )
        .unwrap();
    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let expected = entry_precondition(&page.entries, &target);
    let bytes = b"file replacing an empty directory";
    let prepared = match fixture.store.prepare_upload(
        credential,
        selection(
            &selection_id,
            generation,
            target.clone(),
            bytes,
            Some(expected),
        ),
    ) {
        Ok(prepared) => prepared,
        Err(StoreError::UnsupportedExchange) => {
            assert!(mirror_path(&fixture, &device, &target).is_dir());
            return;
        }
        Err(error) => panic!("prepare returned unexpected error: {error:?}"),
    };
    fixture
        .store
        .put_upload_data(credential, &prepared.upload_id, Body::from(bytes.to_vec()))
        .await
        .unwrap();
    let committed = match fixture.store.commit_upload(credential, &prepared.upload_id) {
        Ok(committed) => committed,
        Err(StoreError::UnsupportedExchange) => {
            assert!(mirror_path(&fixture, &device, &target).is_dir());
            return;
        }
        Err(error) => panic!("commit returned unexpected error: {error:?}"),
    };
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &target)).unwrap(),
        bytes
    );
    assert_eq!(committed.entry.kind, EntryKind::File);
    assert!(committed.entry.revision > expected.revision);
    assert!(
        fixture
            .store
            .get_upload_state(credential, &prepared.upload_id)
            .unwrap()
            .state
            == drip_core::protocol::UploadState::Committed
    );
}

#[tokio::test]
async fn directory_creation_exchanges_a_file_with_its_scanned_revision() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "File to directory");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["tree".to_owned()]).unwrap();
    let selection_id = "b".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::Directory,
    );
    let target = child(&root, "branch");
    let bytes = b"file replaced by a directory";
    upload(
        &fixture.store,
        credential,
        request(
            &selection_id,
            generation,
            target.clone(),
            bytes,
            timestamp(),
        ),
        bytes,
    )
    .await;
    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let expected = entry_precondition(&page.entries, &target);
    let created = match fixture.store.create_directory(
        credential,
        DirectoryCreateRequest {
            selection_id: selection_id.clone(),
            generation,
            source_path: target.clone(),
            expected_entry: Some(expected),
        },
    ) {
        Ok(created) => created,
        Err(StoreError::UnsupportedExchange) => {
            assert_eq!(
                fs::read(mirror_path(&fixture, &device, &target)).unwrap(),
                bytes
            );
            return;
        }
        Err(error) => panic!("directory creation returned unexpected error: {error:?}"),
    };
    assert_eq!(created.entry.kind, EntryKind::Directory);
    assert!(created.entry.revision > expected.revision);
    assert!(mirror_path(&fixture, &device, &target).is_dir());
    assert_eq!(
        fs::read_dir(mirror_path(&fixture, &device, &target))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn nonempty_directory_exchange_is_verified_or_rejected_safely() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Nonempty exchange");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["tree".to_owned()]).unwrap();
    let selection_id = "c".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::Directory,
    );
    let target = child(&root, "branch");
    let child_file = child(&target, "keep.txt");
    let mirror = mirror_path(&fixture, &device, &target);
    fs::create_dir_all(&mirror).unwrap();
    fs::write(mirror.join("keep.txt"), b"keep this entry").unwrap();
    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let expected = entry_precondition(&page.entries, &target);
    let bytes = b"replacement";
    assert!(matches!(
        fixture.store.prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                target.clone(),
                bytes,
                timestamp(),
            )
        ),
        Err(StoreError::Conflict)
    ));
    let prepared = match fixture.store.prepare_upload(
        credential,
        selection(
            &selection_id,
            generation,
            target.clone(),
            bytes,
            Some(expected),
        ),
    ) {
        Ok(prepared) => prepared,
        Err(StoreError::UnsupportedExchange) => {
            assert!(mirror.is_dir());
            assert_eq!(
                fs::read(mirror.join("keep.txt")).unwrap(),
                b"keep this entry"
            );
            return;
        }
        Err(error) => panic!("prepare returned unexpected error: {error:?}"),
    };
    fixture
        .store
        .put_upload_data(credential, &prepared.upload_id, Body::from(bytes.to_vec()))
        .await
        .unwrap();
    match fixture.store.commit_upload(credential, &prepared.upload_id) {
        Ok(committed) => {
            assert_eq!(committed.entry.kind, EntryKind::File);
            assert_eq!(fs::read(&mirror).unwrap(), bytes);
            assert!(!mirror_path(&fixture, &device, &child_file).exists());
        }
        Err(StoreError::UnsupportedExchange) => {
            assert!(mirror.is_dir());
            assert_eq!(
                fs::read(mirror.join("keep.txt")).unwrap(),
                b"keep this entry"
            );
        }
        Err(error) => panic!("commit returned unexpected error: {error:?}"),
    }
}

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}
