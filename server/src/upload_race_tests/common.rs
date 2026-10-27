use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    thread::{self, JoinHandle},
};

use crate::{Store, StoreResult};
use axum::body::Body;
use drip_core::protocol::{
    EnrollmentRequest, Selection, SelectionUpdate, UploadCommitResponse, UploadPrepareRequest,
};
use drip_core::{Blake3Digest, SensitiveString, SourcePath, Timestamp};

pub(super) use checkpoint_gate::CheckpointGate;

mod checkpoint_gate;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

const OLD_BYTES: &[u8] = b"older mirror bytes";
const NEW_BYTES: &[u8] = b"new verified upload";

pub(super) struct Fixture {
    root: PathBuf,
    state_dir: PathBuf,
    mirror_dir: PathBuf,
    device_directory: String,
    pub(super) store: Store,
    pub(super) credential: String,
    pub(super) pc_id: String,
    pub(super) generation: u64,
    pub(super) source: SourcePath,
    pub(super) upload_id: String,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-01/server-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!("{}-upload-race-{suffix}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let state_dir = root.join("state");
        let mirror_dir = root.join("mirror");
        let store = Store::open(&state_dir, &mirror_dir).unwrap();
        let enrollment_token = store.create_enrollment_token().unwrap();
        let device = store
            .enroll(EnrollmentRequest {
                token: SensitiveString::new(enrollment_token),
                label: "Upload race".to_owned(),
            })
            .unwrap();
        let credential = device.device_credential.expose_secret().to_owned();
        let selection_id = "a".repeat(32);
        let source = SourcePath::new('C', vec!["race.bin".to_owned()]).unwrap();
        let generation = store
            .update_selections(
                &credential,
                SelectionUpdate {
                    expected_generation: 0,
                    selections: vec![Selection {
                        id: selection_id.clone(),
                        source: source.clone(),
                        active: true,
                    }],
                },
            )
            .unwrap()
            .generation;
        let old = prepare(
            &store,
            &credential,
            &selection_id,
            generation,
            &source,
            OLD_BYTES,
        );
        store
            .put_upload_data(&credential, &old.upload_id, Body::from(OLD_BYTES.to_vec()))
            .await
            .unwrap();
        store.commit_upload(&credential, &old.upload_id).unwrap();
        assert_eq!(
            std::fs::read(mirror_path(&mirror_dir, &device.mirror_directory, &source)).unwrap(),
            OLD_BYTES
        );
        let replacement = prepare(
            &store,
            &credential,
            &selection_id,
            generation,
            &source,
            NEW_BYTES,
        );
        store
            .put_upload_data(
                &credential,
                &replacement.upload_id,
                Body::from(NEW_BYTES.to_vec()),
            )
            .await
            .unwrap();
        Self {
            root,
            state_dir,
            mirror_dir,
            device_directory: device.mirror_directory,
            store,
            credential,
            pc_id: device.pc_id,
            generation,
            source,
            upload_id: replacement.upload_id,
        }
    }

    pub(super) fn visible_bytes(&self) -> Vec<u8> {
        std::fs::read(mirror_path(
            &self.mirror_dir,
            &self.device_directory,
            &self.source,
        ))
        .unwrap()
    }

    pub(super) fn remove_selection(&self) -> StoreResult<u64> {
        self.store
            .update_selections(
                &self.credential,
                SelectionUpdate {
                    expected_generation: self.generation,
                    selections: Vec::new(),
                },
            )
            .map(|response| response.generation)
    }

    pub(super) fn start_commit(
        &self,
        checkpoint: &'static str,
    ) -> (
        CheckpointGate,
        JoinHandle<StoreResult<UploadCommitResponse>>,
    ) {
        let gate = CheckpointGate::new(checkpoint);
        let store = self.store.clone();
        let credential = self.credential.clone();
        let upload_id = self.upload_id.clone();
        let commit = thread::spawn(move || store.commit_upload(&credential, &upload_id));
        gate.wait_until_entered();
        (gate, commit)
    }

    pub(super) fn run_revoke_cli(&self) -> Output {
        let binary = server_binary();
        Command::new(binary)
            .args(["--state-dir", state_path(&self.state_dir)])
            .args(["--mirror-dir", state_path(&self.mirror_dir)])
            .args(["devices", "revoke", &self.pc_id])
            .output()
            .expect("run separate server CLI")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn prepare(
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
                digest: Blake3Digest::new(*blake3::hash(bytes).as_bytes()),
                last_write: Timestamp::new(1_700_000_000, 123_456_789).unwrap(),
            },
        )
        .unwrap()
}

pub(super) fn assert_cli_busy(output: &Output) {
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("storage is busy"));
}

pub(super) fn assert_cli_success(output: &Output) {
    assert!(
        output.status.success(),
        "CLI revoke failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn assert_old_bytes(fixture: &Fixture) {
    assert_eq!(fixture.visible_bytes(), OLD_BYTES);
}

pub(super) fn assert_new_bytes(fixture: &Fixture) {
    assert_eq!(fixture.visible_bytes(), NEW_BYTES);
}

fn mirror_path(mirror: &Path, directory: &str, source: &SourcePath) -> PathBuf {
    let mut path = mirror.join(directory).join(source.drive().to_string());
    for component in source.components() {
        path.push(component);
    }
    path
}

fn state_path(path: &Path) -> &str {
    path.to_str().expect("test path is UTF-8")
}

fn server_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_server") {
        return PathBuf::from(path);
    }
    std::env::current_exe()
        .expect("test executable path")
        .parent()
        .and_then(Path::parent)
        .expect("Cargo target directory")
        .join(format!("server{}", std::env::consts::EXE_SUFFIX))
}
