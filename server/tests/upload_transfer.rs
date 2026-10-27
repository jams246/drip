mod common;

use std::fs;

use axum::body::Body;
use common::{Fixture, enroll, mirror_path, request, select, upload};
use drip_core::protocol::UploadState;
use drip_core::{SourcePath, Timestamp};
use drip_server::StoreError;

#[tokio::test]
async fn empty_binary_and_multimegabyte_files_publish_with_actual_inventory() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Transfer");
    let credential = device.device_credential.expose_secret();
    let large: Vec<u8> = (0..5 * 1024 * 1024 + 17)
        .map(|index| ((index * 37) % 251) as u8)
        .collect();
    let files = [
        ("empty.bin", Vec::new()),
        ("binary.bin", vec![0, 1, 255, 13, 10, 0, 128]),
        ("large.bin", large),
    ];
    let last_write = Timestamp::new(1_700_000_000, 123_456_789).unwrap();
    for (index, (name, bytes)) in files.iter().enumerate() {
        let source = SourcePath::new('C', vec!["nested".to_owned(), (*name).to_owned()]).unwrap();
        let selection_id = format!("{:032x}", index + 1);
        let generation = select(&fixture.store, credential, &selection_id, source.clone());
        let (_, committed) = upload(
            &fixture.store,
            credential,
            request(&selection_id, generation, source.clone(), bytes, last_write),
            bytes,
        )
        .await;
        assert_eq!(committed.entry.source_path, source);
        assert_eq!(
            fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
            *bytes
        );
        assert_eq!(committed.entry.size, bytes.len() as u64);
        assert_eq!(
            committed.entry.digest,
            Some(drip_core::Blake3Digest::new(
                *blake3::hash(bytes).as_bytes()
            ))
        );
        let page = fixture
            .store
            .list_entries(credential, &selection_id, None)
            .unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].revision, committed.entry.revision);
        assert_eq!(page.entries[0].last_write, Some(last_write));
        assert!(page.next_cursor.is_none());
    }
}

#[tokio::test]
async fn short_excess_and_corrupt_bodies_preserve_existing_destination() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Invalid bodies");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["target.bin".to_owned()]).unwrap();
    let selection_id = "a".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let old = b"last complete destination";
    upload(
        &fixture.store,
        credential,
        request(&selection_id, generation, source.clone(), old, timestamp()),
        old,
    )
    .await;
    let expected = b"replacement bytes";
    let mut corrupt = expected.to_vec();
    corrupt[0] ^= 1;
    let mut excess = expected.to_vec();
    excess.push(0);
    let bodies = [corrupt, expected[..expected.len() - 1].to_vec(), excess];
    for body in bodies {
        let prepared = fixture
            .store
            .prepare_upload(
                credential,
                request(
                    &selection_id,
                    generation,
                    source.clone(),
                    expected,
                    timestamp(),
                ),
            )
            .unwrap();
        assert!(matches!(
            fixture
                .store
                .put_upload_data(credential, &prepared.upload_id, Body::from(body))
                .await,
            Err(StoreError::InvalidInput)
        ));
        assert_eq!(
            fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
            old
        );
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
    }
}

#[tokio::test]
async fn commit_rehashes_staging_and_rejects_a_stale_destination_base() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Fresh verification");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["target.bin".to_owned()]).unwrap();
    let selection_id = "b".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let old = b"original bytes";
    upload(
        &fixture.store,
        credential,
        request(&selection_id, generation, source.clone(), old, timestamp()),
        old,
    )
    .await;
    let expected = b"verified replacement";
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                expected,
                timestamp(),
            ),
        )
        .unwrap();
    fixture
        .store
        .put_upload_data(
            credential,
            &prepared.upload_id,
            Body::from(expected.to_vec()),
        )
        .await
        .unwrap();
    let staged = fixture
        ._root
        .join("mirror/.drip-staging")
        .join(format!("{}.part", prepared.upload_id));
    fs::write(staged, b"tampered staging").unwrap();
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
        old
    );

    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                expected,
                timestamp(),
            ),
        )
        .unwrap();
    fixture
        .store
        .put_upload_data(
            credential,
            &prepared.upload_id,
            Body::from(expected.to_vec()),
        )
        .await
        .unwrap();
    fs::write(mirror_path(&fixture, &device, &source), b"external update").unwrap();
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
        b"external update"
    );
}

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}
