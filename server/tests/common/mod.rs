#![expect(
    dead_code,
    reason = "Shared helpers serve separate integration-test binaries."
)]

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::body::Body;
use drip_core::protocol::{
    EnrollmentRequest, EnrollmentResponse, EntryKind, Selection, SelectionUpdate,
    UploadCommitResponse, UploadPrepareRequest, UploadPrepareResponse,
};
use drip_core::{Blake3Digest, SensitiveString, SourcePath, Timestamp};
use drip_server::Store;
use std::sync::atomic::AtomicI64;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub _root: PathBuf,
    pub store: Store,
}

impl Fixture {
    pub fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-02/server-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = parent.join(format!("{}-{nanos}-{suffix}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("state"), root.join("mirror")).unwrap();
        Self { _root: root, store }
    }

    pub fn with_clock(clock: std::sync::Arc<AtomicI64>) -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-02/server-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = parent.join(format!("{}-clock-{nanos}-{suffix}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let clock = std::sync::Arc::clone(&clock);
        let store = Store::open_with_clock(root.join("state"), root.join("mirror"), move || {
            Ok(clock.load(Ordering::SeqCst))
        })
        .unwrap();
        Self { _root: root, store }
    }
}

pub fn enroll(store: &Store, label: &str) -> EnrollmentResponse {
    let token = store.create_enrollment_token().unwrap();
    store
        .enroll(EnrollmentRequest {
            token: SensitiveString::new(token),
            label: label.to_owned(),
        })
        .unwrap()
}

pub fn select(store: &Store, credential: &str, id: &str, source: SourcePath) -> u64 {
    select_kind(store, credential, id, source, EntryKind::File)
}

pub fn select_kind(
    store: &Store,
    credential: &str,
    id: &str,
    source: SourcePath,
    root_kind: EntryKind,
) -> u64 {
    let current = store.get_selections(credential).unwrap();
    let mut selections = current.selections;
    for selection in &mut selections {
        selection.active = selection.id == id;
        if selection.id == id {
            selection.root_kind = root_kind;
        }
    }
    if let Some(existing) = selections.iter().find(|selection| selection.id == id) {
        assert_eq!(existing.source, source);
    } else {
        selections.push(Selection {
            id: id.to_owned(),
            source,
            root_kind,
            active: true,
        });
    }
    store
        .update_selections(
            credential,
            SelectionUpdate {
                expected_generation: current.generation,
                selections,
            },
        )
        .unwrap()
        .generation
}

pub fn request(
    selection_id: &str,
    generation: u64,
    source_path: SourcePath,
    bytes: &[u8],
    last_write: Timestamp,
) -> UploadPrepareRequest {
    UploadPrepareRequest {
        selection_id: selection_id.to_owned(),
        generation,
        source_path,
        expected_size: bytes.len() as u64,
        digest: Blake3Digest::new(*blake3::hash(bytes).as_bytes()),
        last_write,
        expected_entry: None,
    }
}

pub async fn upload(
    store: &Store,
    credential: &str,
    prepare: UploadPrepareRequest,
    bytes: &[u8],
) -> (UploadPrepareResponse, UploadCommitResponse) {
    let prepared = store.prepare_upload(credential, prepare).unwrap();
    store
        .put_upload_data(credential, &prepared.upload_id, Body::from(bytes.to_vec()))
        .await
        .unwrap();
    let commit = store
        .commit_upload(credential, &prepared.upload_id)
        .unwrap();
    (prepared, commit)
}

pub fn mirror_path(fixture: &Fixture, device: &EnrollmentResponse, source: &SourcePath) -> PathBuf {
    let mut path = fixture
        ._root
        .join("mirror")
        .join(&device.mirror_directory)
        .join(source.drive().to_string());
    for component in source.components() {
        path.push(component);
    }
    path
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self._root);
    }
}
