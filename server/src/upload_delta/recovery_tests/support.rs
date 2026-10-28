use std::{io::Cursor, path::PathBuf, sync::atomic::AtomicU64};

use axum::body::Body;
use drip_core::protocol::UploadPrepareRequest;
use drip_core::{
    Blake3Digest, ChunkConfig, ChunkStream, SourcePath, Timestamp, protocol::UploadPlanRequest,
};
use rusqlite::params;

use crate::Store;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

pub(super) struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn root() -> (PathBuf, TestRoot) {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".verification/phase-03/server-recovery-tests");
    let path = crate::upload_test_support::create_test_dir(&parent, "recovery", &NEXT_ROOT);
    (path.clone(), TestRoot(path))
}

pub(super) async fn commit_delta(
    store: &Store,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source: &SourcePath,
    bytes: &[u8],
) {
    let prepared = prepare(store, credential, selection_id, generation, source, bytes);
    let descriptors = descriptors(bytes);
    let planned = store
        .plan_upload(credential, &prepared.upload_id, plan(descriptors.clone()))
        .unwrap();
    send_missing(
        store,
        credential,
        &prepared.upload_id,
        bytes,
        &descriptors,
        &planned.missing_ordinals,
    )
    .await;
    store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
}

pub(super) fn prepare(
    store: &Store,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source: &SourcePath,
    bytes: &[u8],
) -> drip_core::protocol::UploadPrepareResponse {
    store
        .prepare_upload(
            credential,
            UploadPrepareRequest {
                selection_id: selection_id.to_owned(),
                generation,
                source_path: source.clone(),
                expected_size: bytes.len() as u64,
                digest: digest(bytes),
                last_write: Timestamp::new(1_700_000_000, 123_456_789).unwrap(),
                expected_entry: None,
            },
        )
        .unwrap()
}

pub(super) async fn send_missing(
    store: &Store,
    credential: &str,
    upload_id: &str,
    bytes: &[u8],
    descriptors: &[drip_core::ChunkDescriptor],
    missing: &[u64],
) {
    for ordinal in missing {
        let descriptor = descriptors[usize::try_from(*ordinal).unwrap()];
        let start = usize::try_from(descriptor.offset).unwrap();
        let end = start + usize::try_from(descriptor.length).unwrap();
        store
            .put_upload_chunk(
                credential,
                upload_id,
                *ordinal,
                Body::from(bytes[start..end].to_vec()),
            )
            .await
            .unwrap();
    }
}

pub(super) fn plan(descriptors: Vec<drip_core::ChunkDescriptor>) -> UploadPlanRequest {
    UploadPlanRequest {
        config: ChunkConfig::V1,
        descriptors,
    }
}

pub(super) fn descriptors(bytes: &[u8]) -> Vec<drip_core::ChunkDescriptor> {
    let mut chunks = ChunkStream::new(Cursor::new(bytes));
    let mut descriptors = Vec::new();
    while let Some(chunk) = chunks.next_chunk().unwrap() {
        descriptors.push(chunk.descriptor);
    }
    assert_eq!(chunks.finish().unwrap(), digest(bytes));
    descriptors
}

pub(super) fn digest(bytes: &[u8]) -> Blake3Digest {
    Blake3Digest::new(*blake3::hash(bytes).as_bytes())
}

pub(super) fn metadata(store: &Store, pc_id: &str, source: &SourcePath) -> (i64, String) {
    let connection = store.connect().unwrap();
    let source_json = serde_json::to_string(source).unwrap();
    connection
        .query_row(
            "SELECT revision, digest FROM entry_metadata WHERE pc_id = ?1 AND source_json = ?2",
            params![pc_id, source_json],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

pub(super) fn manifest_count(
    store: &Store,
    pc_id: &str,
    source: &SourcePath,
    revision: i64,
) -> i64 {
    let connection = store.connect().unwrap();
    let source_json = serde_json::to_string(source).unwrap();
    connection
        .query_row(
            "SELECT COUNT(*) FROM chunk_manifests \
             WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3 AND complete = 1",
            params![pc_id, source_json, revision],
            |row| row.get(0),
        )
        .unwrap()
}

pub(super) fn pseudo_random(length: usize) -> Vec<u8> {
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

pub(super) fn mirror_path(root: &std::path::Path, directory: &str, source: &SourcePath) -> PathBuf {
    let mut path = root
        .join("mirror")
        .join(directory)
        .join(source.drive().to_string());
    for component in source.components() {
        path.push(component);
    }
    path
}
