mod common;

use std::{
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::body::Body;
use common::{Fixture, enroll, mirror_path, request, select, upload};
use drip_core::protocol::UploadPlanRequest;
use drip_core::{Blake3Digest, ChunkConfig, ChunkStream, SourcePath, Timestamp};
use drip_server::StoreError;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn legacy_baseline_reuses_ranges_and_retries_keep_their_original_result() {
    let fixture = fixture();
    let device = enroll(&fixture.store, "Delta baseline");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["delta.bin".to_owned()]).unwrap();
    let selection_id = "d".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let last_write = timestamp();
    let baseline = pseudo_random(4 * 1024 * 1024);
    upload(
        &fixture.store,
        credential,
        request(
            &selection_id,
            generation,
            source.clone(),
            &baseline,
            last_write,
        ),
        &baseline,
    )
    .await;
    let mut changed = baseline.clone();
    let edit_offset = changed.len() / 2;
    changed[edit_offset] ^= 1;
    let descriptors = descriptors(&changed);
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                &changed,
                last_write,
            ),
        )
        .unwrap();
    let plan = UploadPlanRequest {
        config: ChunkConfig::V1,
        descriptors: descriptors.clone(),
    };
    let original = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan.clone())
        .unwrap();
    assert!(!original.missing_ordinals.is_empty());
    assert!(original.missing_ordinals.len() < descriptors.len());
    assert_eq!(
        fixture
            .store
            .plan_upload(credential, &prepared.upload_id, plan.clone())
            .unwrap(),
        original
    );
    assert!(matches!(
        fixture
            .store
            .put_upload_data(credential, &prepared.upload_id, Body::empty())
            .await,
        Err(StoreError::Conflict)
    ));

    let first = original.missing_ordinals[0];
    let first_descriptor = descriptors[usize::try_from(first).unwrap()];
    let first_bytes = chunk_bytes(&changed, first_descriptor);
    let mut corrupt = first_bytes.clone();
    corrupt[0] ^= 1;
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                first,
                Body::from(corrupt.clone()),
            )
            .await,
        Err(StoreError::CorruptContent)
    ));
    fixture
        .store
        .put_upload_chunk(
            credential,
            &prepared.upload_id,
            first,
            Body::from(first_bytes.clone()),
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(credential, &prepared.upload_id, first, Body::from(corrupt),)
            .await,
        Err(StoreError::Conflict)
    ));
    for ordinal in original.missing_ordinals.iter().copied().skip(1) {
        fixture
            .store
            .put_upload_chunk(
                credential,
                &prepared.upload_id,
                ordinal,
                Body::from(chunk_bytes(
                    &changed,
                    descriptors[usize::try_from(ordinal).unwrap()],
                )),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        fixture
            .store
            .plan_upload(credential, &prepared.upload_id, plan.clone())
            .unwrap(),
        original
    );
    let mut conflicting = plan;
    conflicting.descriptors[usize::try_from(first).unwrap()].digest = Blake3Digest::new([0; 32]);
    assert!(matches!(
        fixture
            .store
            .plan_upload(credential, &prepared.upload_id, conflicting),
        Err(StoreError::Conflict)
    ));
    fixture
        .store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
    assert_eq!(
        std::fs::read(mirror_path(&fixture, &device, &source)).unwrap(),
        changed
    );
}

#[tokio::test]
async fn empty_file_uses_an_empty_plan_and_publishes() {
    let fixture = fixture();
    let device = enroll(&fixture.store, "Empty delta");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["empty.bin".to_owned()]).unwrap();
    let selection_id = "e".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let empty = [];
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                &empty,
                timestamp(),
            ),
        )
        .unwrap();
    let result = fixture
        .store
        .plan_upload(
            credential,
            &prepared.upload_id,
            UploadPlanRequest {
                config: ChunkConfig::V1,
                descriptors: Vec::new(),
            },
        )
        .unwrap();
    assert!(result.missing_ordinals.is_empty());
    fixture
        .store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
    assert!(
        std::fs::read(mirror_path(&fixture, &device, &source))
            .unwrap()
            .is_empty()
    );
}

fn descriptors(bytes: &[u8]) -> Vec<drip_core::ChunkDescriptor> {
    let mut chunks = ChunkStream::new(Cursor::new(bytes));
    let mut descriptors = Vec::new();
    while let Some(chunk) = chunks.next_chunk().unwrap() {
        descriptors.push(chunk.descriptor);
    }
    assert_eq!(
        chunks.finish().unwrap(),
        Blake3Digest::new(*blake3::hash(bytes).as_bytes())
    );
    descriptors
}

fn chunk_bytes(bytes: &[u8], descriptor: drip_core::ChunkDescriptor) -> Vec<u8> {
    let start = usize::try_from(descriptor.offset).unwrap();
    let end = start + usize::try_from(descriptor.length).unwrap();
    bytes[start..end].to_vec()
}

fn pseudo_random(length: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}

fn fixture() -> Fixture {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".verification/phase-03/server-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = parent.join(format!("{}-{id}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let store = drip_server::Store::open(root.join("state"), root.join("mirror")).unwrap();
    Fixture { _root: root, store }
}
