use std::{
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::body::Body;
use drip_core::protocol::{
    EnrollmentRequest, EnrollmentResponse, EntryKind, Selection, SelectionUpdate,
    UploadPlanRequest, UploadPlanResponse, UploadPrepareRequest, UploadPrepareResponse,
};
use drip_core::{
    Blake3Digest, ChunkConfig, ChunkDescriptor, ChunkStream, SensitiveString, SourcePath, Timestamp,
};
use drip_server::Store;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub root: PathBuf,
    pub store: Store,
}

impl Fixture {
    pub fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-03/protocol-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!("{}-{suffix}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("state"), root.join("mirror")).unwrap();
        Self { root, store }
    }

    pub fn enroll(&self, label: &str) -> EnrollmentResponse {
        let token = self.store.create_enrollment_token().unwrap();
        self.store
            .enroll(EnrollmentRequest {
                token: SensitiveString::new(token),
                label: label.to_owned(),
            })
            .unwrap()
    }

    pub fn select(&self, credential: &str, id: &str, source: SourcePath) -> u64 {
        self.store
            .update_selections(
                credential,
                SelectionUpdate {
                    expected_generation: 0,
                    selections: vec![Selection {
                        id: id.to_owned(),
                        source,
                        root_kind: EntryKind::File,
                        active: true,
                    }],
                },
            )
            .unwrap()
            .generation
    }

    pub fn mirror_path(&self, device: &EnrollmentResponse, source: &SourcePath) -> PathBuf {
        let mut path = self.root.join("mirror").join(&device.mirror_directory);
        path.push(source.drive().to_string());
        for component in source.components() {
            path.push(component);
        }
        path
    }
}

pub fn prepare(
    store: &Store,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source: SourcePath,
    bytes: &[u8],
) -> UploadPrepareResponse {
    store
        .prepare_upload(
            credential,
            prepare_request(selection_id, generation, source, bytes),
        )
        .unwrap()
}

pub fn prepare_request(
    selection_id: &str,
    generation: u64,
    source: SourcePath,
    bytes: &[u8],
) -> UploadPrepareRequest {
    UploadPrepareRequest {
        selection_id: selection_id.to_owned(),
        generation,
        source_path: source,
        expected_size: bytes.len() as u64,
        digest: digest(bytes),
        last_write: timestamp(),
        expected_entry: None,
    }
}

pub async fn upload_legacy(
    store: &Store,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source: SourcePath,
    bytes: &[u8],
) {
    let prepared = prepare(store, credential, selection_id, generation, source, bytes);
    store
        .put_upload_data(credential, &prepared.upload_id, Body::from(bytes.to_vec()))
        .await
        .unwrap();
    store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
}

pub fn descriptors(bytes: &[u8]) -> Vec<ChunkDescriptor> {
    let mut stream = ChunkStream::new(Cursor::new(bytes));
    let mut descriptors = Vec::new();
    while let Some(chunk) = stream.next_chunk().unwrap() {
        descriptors.push(chunk.descriptor);
    }
    assert_eq!(stream.finish().unwrap(), digest(bytes));
    descriptors
}

pub fn plan(descriptors: Vec<ChunkDescriptor>) -> UploadPlanRequest {
    UploadPlanRequest {
        config: ChunkConfig::V1,
        descriptors,
    }
}

pub fn chunk_bytes(bytes: &[u8], descriptor: ChunkDescriptor) -> Vec<u8> {
    let start = usize::try_from(descriptor.offset).unwrap();
    let end = start + usize::try_from(descriptor.length).unwrap();
    bytes[start..end].to_vec()
}

pub async fn send_missing(
    store: &Store,
    credential: &str,
    upload_id: &str,
    bytes: &[u8],
    descriptors: &[ChunkDescriptor],
    result: &UploadPlanResponse,
) -> u64 {
    let mut sent = 0;
    for ordinal in result.missing_ordinals.iter().copied() {
        let descriptor = descriptors[usize::try_from(ordinal).unwrap()];
        let literal = chunk_bytes(bytes, descriptor);
        sent += literal.len() as u64;
        store
            .put_upload_chunk(credential, upload_id, ordinal, Body::from(literal))
            .await
            .unwrap();
    }
    sent
}

pub async fn run_delta(
    fixture: &Fixture,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source: &SourcePath,
    bytes: &[u8],
    mirror: &std::path::Path,
) -> (u64, u64, u64) {
    let prepare_request = prepare_request(selection_id, generation, source.clone(), bytes);
    let mut request_json_bytes = serde_json::to_vec(&prepare_request).unwrap().len() as u64;
    let prepared = fixture
        .store
        .prepare_upload(credential, prepare_request)
        .unwrap();
    let mut response_json_bytes = serde_json::to_vec(&prepared).unwrap().len() as u64;
    let descriptors = descriptors(bytes);
    let plan_request = plan(descriptors.clone());
    request_json_bytes += serde_json::to_vec(&plan_request).unwrap().len() as u64;
    let result = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan_request)
        .unwrap();
    response_json_bytes += serde_json::to_vec(&result).unwrap().len() as u64;
    let sent = send_missing(
        &fixture.store,
        credential,
        &prepared.upload_id,
        bytes,
        &descriptors,
        &result,
    )
    .await;
    let committed = fixture
        .store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
    response_json_bytes += serde_json::to_vec(&committed).unwrap().len() as u64;
    let published = std::fs::read(mirror).unwrap();
    assert_eq!(published, bytes);
    assert_eq!(digest(&published), digest(bytes));
    (sent, request_json_bytes, response_json_bytes)
}

pub fn digest(bytes: &[u8]) -> Blake3Digest {
    Blake3Digest::new(*blake3::hash(bytes).as_bytes())
}

pub fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}

pub fn pseudo_random(length: usize) -> Vec<u8> {
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
