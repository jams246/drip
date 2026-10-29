mod common;

use std::{io::Cursor, path::PathBuf};

use common::{enroll, request, select, unique_fixture_directory, upload};
use drip_core::protocol::UploadPlanRequest;
use drip_core::{Blake3Digest, ChunkConfig, ChunkStream, SourcePath, Timestamp};
use rusqlite::params;

struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn incomplete_manifest_rebuilds_and_corrupt_range_is_missing() {
    let root = test_root();
    let _cleanup = TestRoot(root.clone());
    let store = drip_server::Store::open(root.join("state"), root.join("mirror")).unwrap();
    let device = enroll(&store, "Manifest recovery");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["manifest.bin".to_owned()]).unwrap();
    let selection_id = "f".repeat(32);
    let generation = select(&store, credential, &selection_id, source.clone());
    let bytes = pseudo_random(4 * 1024 * 1024);
    let last_write = Timestamp::new(1_700_000_000, 123_456_789).unwrap();
    upload(
        &store,
        credential,
        request(
            &selection_id,
            generation,
            source.clone(),
            &bytes,
            last_write,
        ),
        &bytes,
    )
    .await;
    let descriptors = descriptors(&bytes);
    let first = store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                &bytes,
                last_write,
            ),
        )
        .unwrap();
    let first_plan = plan(&descriptors);
    assert!(
        store
            .plan_upload(credential, &first.upload_id, first_plan.clone())
            .unwrap()
            .missing_ordinals
            .is_empty()
    );
    let revision = store
        .commit_upload(credential, &first.upload_id)
        .unwrap()
        .entry
        .revision;

    let source_json = serde_json::to_string(&source).unwrap();
    let database = rusqlite::Connection::open(root.join("state/drip.sqlite3")).unwrap();
    let removed = database
        .execute(
            "DELETE FROM file_chunks WHERE pc_id = ?1 AND source_json = ?2 \
             AND revision = ?3 AND ordinal = 0",
            params![device.pc_id, source_json, i64::try_from(revision).unwrap()],
        )
        .unwrap();
    assert_eq!(removed, 1);

    let second = store
        .prepare_upload(
            credential,
            request(
                &selection_id,
                generation,
                source.clone(),
                &bytes,
                last_write,
            ),
        )
        .unwrap();
    assert!(
        store
            .plan_upload(credential, &second.upload_id, first_plan)
            .unwrap()
            .missing_ordinals
            .is_empty()
    );
    let revision = store
        .commit_upload(credential, &second.upload_id)
        .unwrap()
        .entry
        .revision;

    let corrupt_digest = Blake3Digest::new([0xa5; 32]);
    assert_ne!(descriptors[0].digest, corrupt_digest);
    let changed = database
        .execute(
            "UPDATE file_chunks SET digest = ?1 WHERE pc_id = ?2 AND source_json = ?3 \
             AND revision = ?4 AND ordinal = 0",
            params![
                corrupt_digest.to_hex(),
                device.pc_id,
                source_json,
                i64::try_from(revision).unwrap(),
            ],
        )
        .unwrap();
    assert_eq!(changed, 1);

    let mut untrusted = descriptors;
    untrusted[0].digest = corrupt_digest;
    let third = store
        .prepare_upload(
            credential,
            request(&selection_id, generation, source, &bytes, last_write),
        )
        .unwrap();
    let response = store
        .plan_upload(credential, &third.upload_id, plan(&untrusted))
        .unwrap();
    assert_eq!(response.missing_ordinals, [0]);
}

fn plan(descriptors: &[drip_core::ChunkDescriptor]) -> UploadPlanRequest {
    UploadPlanRequest {
        config: ChunkConfig::V1,
        descriptors: descriptors.to_vec(),
    }
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

fn test_root() -> PathBuf {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".verification/phase-03/server-manifest-tests");
    unique_fixture_directory(&parent, "delta-manifest")
}
