mod common;

use std::fs;

use axum::body::Body;
use common::{Fixture, enroll, mirror_path, request, select, upload};
use drip_core::protocol::UploadState;
use drip_core::{SourcePath, Timestamp};
use drip_server::StoreError;

#[tokio::test]
async fn foreign_session_ids_are_absent_and_abort_is_idempotent() {
    let fixture = Fixture::new();
    let owner = enroll(&fixture.store, "Owner");
    let other = enroll(&fixture.store, "Other");
    let source = SourcePath::new('C', vec!["private.bin".to_owned()]).unwrap();
    let selection_id = "c".repeat(32);
    let generation = select(
        &fixture.store,
        owner.device_credential.expose_secret(),
        &selection_id,
        source.clone(),
    );
    let prepared = fixture
        .store
        .prepare_upload(
            owner.device_credential.expose_secret(),
            request(&selection_id, generation, source, b"private", timestamp()),
        )
        .unwrap();
    let foreign = other.device_credential.expose_secret();
    assert!(matches!(
        fixture.store.get_upload_state(foreign, &prepared.upload_id),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        fixture.store.abort_upload(foreign, &prepared.upload_id),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        fixture
            .store
            .put_upload_data(foreign, &prepared.upload_id, Body::from("private"))
            .await,
        Err(StoreError::NotFound)
    ));
    let credential = owner.device_credential.expose_secret();
    fixture
        .store
        .abort_upload(credential, &prepared.upload_id)
        .unwrap();
    fixture
        .store
        .abort_upload(credential, &prepared.upload_id)
        .unwrap();
    assert_eq!(
        fixture
            .store
            .get_upload_state(credential, &prepared.upload_id)
            .unwrap()
            .state,
        UploadState::Aborted
    );
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::Conflict)
    ));
}

#[tokio::test]
async fn selection_barrier_aborts_old_generation_and_allows_fresh_prepare() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Selection barrier");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["file.bin".to_owned()]).unwrap();
    let selection_id = "d".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                b"file",
                timestamp(),
            ),
        )
        .unwrap();
    assert_eq!(
        fixture
            .store
            .update_selections(
                credential,
                drip_core::protocol::SelectionUpdate {
                    expected_generation: generation,
                    selections: fixture.store.get_selections(credential).unwrap().selections,
                },
            )
            .unwrap()
            .generation,
        generation + 1
    );
    assert_eq!(
        fixture
            .store
            .get_upload_state(credential, &prepared.upload_id)
            .unwrap()
            .state,
        UploadState::Aborted
    );
    let fresh = fixture
        .store
        .prepare_upload(
            credential,
            request(&selection_id, generation + 1, source, b"file", timestamp()),
        )
        .unwrap();
    assert_ne!(prepared.upload_id, fresh.upload_id);
}

#[tokio::test]
async fn revoked_device_cannot_publish_prepared_bytes() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Revocation");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["file.bin".to_owned()]).unwrap();
    let selection_id = "e".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let old = b"old visible file";
    upload(
        &fixture.store,
        credential,
        request(&selection_id, generation, source.clone(), old, timestamp()),
        old,
    )
    .await;
    let new = b"must not publish";
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(&selection_id, generation, source.clone(), new, timestamp()),
        )
        .unwrap();
    fixture
        .store
        .put_upload_data(credential, &prepared.upload_id, Body::from(new.to_vec()))
        .await
        .unwrap();
    fixture.store.revoke_device(&device.pc_id).unwrap();
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::InvalidCredential)
    ));
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
        old
    );
}

#[cfg(unix)]
#[tokio::test]
async fn ancestor_symlink_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Symlink confinement");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["linked".to_owned(), "escape.bin".to_owned()]).unwrap();
    let selection_id = "f".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let outside = fixture._root.join("outside");
    fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("escape.bin");
    fs::write(&sentinel, b"unchanged").unwrap();
    let drive = fixture
        ._root
        .join("mirror")
        .join(&device.mirror_directory)
        .join("C");
    fs::create_dir(&drive).unwrap();
    symlink(&outside, drive.join("linked")).unwrap();
    assert!(matches!(
        fixture.store.prepare_upload(
            credential,
            request(&selection_id, generation, source, b"intrusion", timestamp()),
        ),
        Err(StoreError::Conflict)
    ));
    assert_eq!(fs::read(sentinel).unwrap(), b"unchanged");
}

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}
