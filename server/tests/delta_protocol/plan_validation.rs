use drip_core::protocol::UploadPrepareRequest;
use drip_core::protocol::{UploadPlanRequest, UploadPlanValidationError};
use drip_core::{
    Blake3Digest, CHUNK_MAX_SIZE, CHUNK_MIN_SIZE, CHUNK_PLAN_MAX_BODY_BYTES,
    CHUNK_PLAN_MAX_DESCRIPTORS, ChunkConfig, ChunkDescriptor, SourcePath, Timestamp,
};
use drip_server::StoreError;

use super::support::{Fixture, digest, plan, prepare, pseudo_random};

#[tokio::test]
async fn malformed_plan_shapes_and_versions_fail_without_advancing_the_cursor() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Rejected pages");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["bad-plan.bin".to_owned()]).unwrap();
    let selection_id = "c".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let bytes = pseudo_random(2 * CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source,
        &bytes,
    );
    let half = CHUNK_MIN_SIZE as usize;
    let first = descriptor(0, 0, CHUNK_MIN_SIZE, &bytes[..half]);
    let second = descriptor(1, CHUNK_MIN_SIZE as u64, CHUNK_MIN_SIZE, &bytes[half..]);
    let mut invalid = vec![
        plan(vec![descriptor(1, 0, CHUNK_MIN_SIZE, &bytes[..half])]),
        plan(vec![descriptor(0, 1, CHUNK_MIN_SIZE, &bytes[..half])]),
        plan(vec![descriptor(0, 0, 0, &[])]),
        plan(vec![descriptor(0, 0, 1, &bytes[..1])]),
        plan(vec![descriptor(0, 0, CHUNK_MAX_SIZE + 1, &bytes)]),
        plan(vec![
            first,
            descriptor(1, CHUNK_MIN_SIZE as u64 - 1, CHUNK_MIN_SIZE, &bytes[half..]),
        ]),
        plan(vec![descriptor(0, 0, CHUNK_MAX_SIZE, &bytes)]),
        plan(vec![
            first,
            second,
            descriptor(2, 2 * CHUNK_MIN_SIZE as u64, CHUNK_MIN_SIZE, &bytes[..half]),
        ]),
    ];
    invalid.push(UploadPlanRequest {
        config: ChunkConfig {
            chunking_version: 2,
        },
        descriptors: vec![descriptor(0, 0, CHUNK_MIN_SIZE, &bytes[..half])],
    });
    for (index, request) in invalid.into_iter().enumerate() {
        let result = fixture
            .store
            .plan_upload(credential, &prepared.upload_id, request);
        assert!(
            matches!(result, Err(StoreError::InvalidInput)),
            "request {index}: {result:?}"
        );
    }

    let too_many = (0..=drip_core::CHUNK_PLAN_MAX_DESCRIPTORS)
        .map(|ordinal| {
            descriptor(
                ordinal as u64,
                ordinal as u64 * u64::from(CHUNK_MIN_SIZE),
                CHUNK_MIN_SIZE,
                &bytes[..half],
            )
        })
        .collect();
    assert!(matches!(
        fixture
            .store
            .plan_upload(credential, &prepared.upload_id, plan(too_many)),
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        UploadPlanRequest {
            config: ChunkConfig::V1,
            descriptors: vec![descriptor(0, u64::MAX - 2, CHUNK_MIN_SIZE, &bytes[..half])],
        }
        .validate(u64::MAX, 0, u64::MAX - 2),
        Err(UploadPlanValidationError::RangeOverflow)
    ));
    fixture
        .store
        .abort_upload(credential, &prepared.upload_id)
        .unwrap();
}

fn descriptor(ordinal: u64, offset: u64, length: u32, bytes: &[u8]) -> ChunkDescriptor {
    ChunkDescriptor {
        ordinal,
        offset,
        length,
        digest: digest(bytes),
    }
}

#[tokio::test]
async fn full_1024_descriptor_page_and_maximum_chunk_are_accepted() {
    let fixture = Fixture::new();
    let _device = fixture.enroll("Maximum page");
    let credential = _device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["max-page.bin".to_owned()]).unwrap();
    let selection_id = "4".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let mut offset = 0u64;
    let descriptors: Vec<_> = (0..CHUNK_PLAN_MAX_DESCRIPTORS)
        .map(|ordinal| {
            let length = if ordinal == 0 {
                CHUNK_MAX_SIZE
            } else {
                CHUNK_MIN_SIZE
            };
            let descriptor = ChunkDescriptor {
                ordinal: ordinal as u64,
                offset,
                length,
                digest: Blake3Digest::new([0; 32]),
            };
            offset += u64::from(length);
            descriptor
        })
        .collect();
    let request = UploadPlanRequest {
        config: ChunkConfig::V1,
        descriptors,
    };
    assert!(serde_json::to_vec(&request).unwrap().len() <= CHUNK_PLAN_MAX_BODY_BYTES);
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            UploadPrepareRequest {
                selection_id,
                generation,
                source_path: source,
                expected_size: offset,
                digest: Blake3Digest::new([0; 32]),
                last_write: Timestamp::new(1_700_000_000, 0).unwrap(),
                expected_entry: None,
            },
        )
        .unwrap();
    let response = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, request)
        .unwrap();
    assert_eq!(response.missing_ordinals.len(), CHUNK_PLAN_MAX_DESCRIPTORS);
    fixture
        .store
        .abort_upload(credential, &prepared.upload_id)
        .unwrap();
}
