use std::{
    path::PathBuf,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};

use cap_std::{ambient_authority, fs::Dir};
use drip_core::protocol::UploadState;
use drip_core::{Blake3Digest, SourcePath, Timestamp};

use crate::{
    Store,
    upload_sessions::{UploadSession, UploadSessions},
};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

#[test]
fn delayed_generation_cleanup_cannot_abort_a_newer_session() {
    let root = test_root("generation");
    let mirror_path = root.join("mirror");
    std::fs::create_dir_all(&mirror_path).unwrap();
    let mirror = Arc::new(Dir::open_ambient_dir(&mirror_path, ambient_authority()).unwrap());
    let sessions = UploadSessions::new(mirror);
    let stage_name = sessions
        .create_stage("33333333333333333333333333333333")
        .unwrap();
    sessions
        .reserve(session(
            "33333333333333333333333333333333",
            3,
            100,
            stage_name,
        ))
        .unwrap();
    sessions.abort_obsolete("device", 2, 101);
    assert_eq!(
        sessions
            .get("33333333333333333333333333333333", "device", 101)
            .unwrap()
            .lock()
            .unwrap()
            .state,
        UploadState::Active
    );
    sessions.abort_obsolete("device", 4, 102);
    assert_eq!(
        sessions
            .get("33333333333333333333333333333333", "device", 102)
            .unwrap()
            .lock()
            .unwrap()
            .state,
        UploadState::Aborted
    );
    drop(sessions);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn daemon_startup_cleans_only_staging_and_store_open_does_not() {
    let root = test_root("startup");
    let state = root.join("state");
    let mirror = root.join("mirror");
    let store = Store::open(&state, &mirror).unwrap();
    let stage_name = store
        .uploads
        .create_stage("44444444444444444444444444444444")
        .unwrap();
    let stage_path = mirror.join(".drip-staging").join(&stage_name);
    std::fs::write(&stage_path, b"abandoned staging").unwrap();
    let committed_path = mirror.join("committed-file.bin");
    std::fs::write(&committed_path, b"committed bytes").unwrap();
    let _cli_store = Store::open(&state, &mirror).unwrap();
    assert!(stage_path.exists());
    store.prepare_daemon().unwrap();
    assert!(!stage_path.exists());
    assert_eq!(std::fs::read(committed_path).unwrap(), b"committed bytes");
    drop(_cli_store);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

fn session(id: &str, generation: u64, now: i64, stage_name: String) -> UploadSession {
    UploadSession {
        id: id.to_owned(),
        pc_id: "device".to_owned(),
        selection_id: "11111111111111111111111111111111".to_owned(),
        generation,
        source_path: SourcePath::new('C', vec!["file.bin".to_owned()]).unwrap(),
        expected_size: 0,
        digest: Blake3Digest::new(*blake3::hash(&[]).as_bytes()),
        last_write: Timestamp::new(0, 0).unwrap(),
        base_revision: None,
        base_snapshot: None,
        stage_name,
        state: UploadState::Active,
        last_progress: now,
        receiving: false,
        committing: false,
        has_data: false,
    }
}

fn test_root(name: &str) -> PathBuf {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".verification/phase-01/server-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let suffix = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    let root = parent.join(format!("{}-{name}-{suffix}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    root
}
