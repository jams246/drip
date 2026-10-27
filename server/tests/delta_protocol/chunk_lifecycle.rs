use axum::body::Body;
use drip_core::{CHUNK_MIN_SIZE, ChunkDescriptor, SourcePath};
use drip_server::StoreError;

use super::support::{Fixture, digest, plan, prepare, pseudo_random};

#[tokio::test]
async fn missing_chunk_page_gates_retries_and_commit_until_coverage_is_complete() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Chunk protocol");
    let other = fixture.enroll("Other device");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["paged.bin".to_owned()]).unwrap();
    let selection_id = "d".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let bytes = pseudo_random(2 * CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &bytes,
    );
    let half = CHUNK_MIN_SIZE as usize;
    let chunks = [
        descriptor(0, 0, CHUNK_MIN_SIZE, &bytes[..half]),
        descriptor(1, CHUNK_MIN_SIZE as u64, CHUNK_MIN_SIZE, &bytes[half..]),
    ];
    let first_page = plan(vec![chunks[0]]);
    let initial = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, first_page.clone())
        .unwrap();
    assert_eq!(initial.missing_ordinals, [0]);
    assert_eq!(
        fixture
            .store
            .plan_upload(credential, &prepared.upload_id, first_page)
            .unwrap(),
        initial
    );
    assert!(matches!(
        fixture
            .store
            .plan_upload(credential, &prepared.upload_id, plan(vec![chunks[1]])),
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        fixture.store.plan_upload(
            other.device_credential.expose_secret(),
            &prepared.upload_id,
            plan(vec![chunks[0]])
        ),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                1,
                Body::from(bytes[..half].to_vec())
            )
            .await,
        Err(StoreError::InvalidInput)
    ));

    let mut wrong_hash = bytes[..half].to_vec();
    wrong_hash[0] ^= 1;
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(credential, &prepared.upload_id, 0, Body::from(wrong_hash))
            .await,
        Err(StoreError::CorruptContent)
    ));
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                0,
                Body::from(bytes[..half - 1].to_vec())
            )
            .await,
        Err(StoreError::CorruptContent)
    ));
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                0,
                Body::from([bytes[..half].to_vec(), vec![0]].concat())
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    let literal = bytes[..half].to_vec();
    fixture
        .store
        .put_upload_chunk(
            credential,
            &prepared.upload_id,
            0,
            Body::from(literal.clone()),
        )
        .await
        .unwrap();
    fixture
        .store
        .put_upload_chunk(
            credential,
            &prepared.upload_id,
            0,
            Body::from(literal.clone()),
        )
        .await
        .unwrap();
    let mut changed_retry = literal;
    changed_retry[0] ^= 1;
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                0,
                Body::from(changed_retry)
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let second = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(vec![chunks[1]]))
        .unwrap();
    assert_eq!(second.missing_ordinals, [1]);
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::Conflict)
    ));
    fixture
        .store
        .put_upload_chunk(
            credential,
            &prepared.upload_id,
            1,
            Body::from(bytes[half..].to_vec()),
        )
        .await
        .unwrap();
    fixture
        .store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.mirror_path(&device, &source)).unwrap(),
        bytes
    );
}

#[tokio::test]
async fn final_chunk_may_be_shorter_than_the_minimum() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Short final chunk");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["short-final.bin".to_owned()]).unwrap();
    let selection_id = "e".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let bytes = pseudo_random(CHUNK_MIN_SIZE as usize + 7);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &bytes,
    );
    let chunks = vec![
        descriptor(0, 0, CHUNK_MIN_SIZE, &bytes[..CHUNK_MIN_SIZE as usize]),
        descriptor(
            1,
            u64::from(CHUNK_MIN_SIZE),
            7,
            &bytes[CHUNK_MIN_SIZE as usize..],
        ),
    ];
    let result = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(chunks.clone()))
        .unwrap();
    assert_eq!(result.missing_ordinals, [0, 1]);
    for ordinal in [0, 1] {
        let descriptor = chunks[ordinal as usize];
        let start = descriptor.offset as usize;
        let end = start + descriptor.length as usize;
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                ordinal,
                Body::from(bytes[start..end].to_vec()),
            )
            .await
            .unwrap();
    }
    fixture
        .store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.mirror_path(&device, &source)).unwrap(),
        bytes
    );
}

fn descriptor(ordinal: u64, offset: u64, length: u32, bytes: &[u8]) -> ChunkDescriptor {
    ChunkDescriptor {
        ordinal,
        offset,
        length,
        digest: digest(bytes),
    }
}
