#[cfg(target_os = "linux")]
use std::fs::{self, File, FileTimes};

use super::common::{Fixture, assert_cli_success, assert_old_bytes};
use crate::StoreError;
use drip_core::protocol::UploadState;

#[tokio::test]
async fn selection_removal_before_writer_lock_fences_staged_commit() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_writer_lock");
    assert_eq!(fixture.remove_selection().unwrap(), fixture.generation + 1);
    gate.release();
    assert!(matches!(commit.join().unwrap(), Err(StoreError::Conflict)));
    assert_eq!(
        fixture
            .store
            .get_upload_state(&fixture.credential, &fixture.upload_id)
            .unwrap()
            .state,
        UploadState::Aborted
    );
    assert_old_bytes(&fixture);
}

#[tokio::test]
async fn cli_revocation_before_writer_lock_fences_staged_commit() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_writer_lock");
    assert_cli_success(&fixture.run_revoke_cli());
    gate.release();
    assert!(matches!(
        commit.join().unwrap(),
        Err(StoreError::InvalidCredential)
    ));
    assert!(!fixture.store.list_devices().unwrap()[0].active);
    assert_old_bytes(&fixture);
}

#[tokio::test]
async fn session_abort_before_writer_lock_fences_staged_commit() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_writer_lock");
    fixture
        .store
        .abort_upload(&fixture.credential, &fixture.upload_id)
        .unwrap();
    gate.release();
    assert!(matches!(commit.join().unwrap(), Err(StoreError::Conflict)));
    assert_eq!(
        fixture
            .store
            .get_upload_state(&fixture.credential, &fixture.upload_id)
            .unwrap()
            .state,
        UploadState::Aborted
    );
    assert_old_bytes(&fixture);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn writer_lock_rejects_same_size_mtime_restored_destination_change() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_writer_lock");
    let path = super::common::mirror_path(
        &fixture.mirror_dir,
        &fixture.device_directory,
        &fixture.source,
    );
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let changed = b"newer mirror bytes";
    fs::write(&path, changed).unwrap();
    File::open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    gate.release();
    assert!(matches!(commit.join().unwrap(), Err(StoreError::Conflict)));
    assert_eq!(fs::read(path).unwrap(), changed);
}
