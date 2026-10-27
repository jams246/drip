use super::common::{Fixture, assert_cli_busy, assert_cli_success, assert_new_bytes};
use crate::StoreError;
use drip_core::protocol::UploadState;

#[tokio::test]
async fn selection_removal_waits_for_visible_publication() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_rename");
    assert!(matches!(fixture.remove_selection(), Err(StoreError::Busy)));
    gate.release();
    assert!(commit.join().unwrap().is_ok());
    assert_eq!(fixture.remove_selection().unwrap(), fixture.generation + 1);
    assert!(matches!(
        fixture
            .store
            .commit_upload(&fixture.credential, &fixture.upload_id),
        Err(StoreError::Conflict)
    ));
    assert_new_bytes(&fixture);
}

#[tokio::test]
async fn cli_revocation_waits_for_visible_publication() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_rename");
    assert_cli_busy(&fixture.run_revoke_cli());
    gate.release();
    assert!(commit.join().unwrap().is_ok());
    let output = fixture.run_revoke_cli();
    assert_cli_success(&output);
    assert!(matches!(
        fixture
            .store
            .commit_upload(&fixture.credential, &fixture.upload_id),
        Err(StoreError::InvalidCredential)
    ));
    assert!(!fixture.store.list_devices().unwrap()[0].active);
    assert_new_bytes(&fixture);
}

#[tokio::test]
async fn session_abort_waits_for_visible_publication() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_rename");
    assert!(matches!(
        fixture
            .store
            .abort_upload(&fixture.credential, &fixture.upload_id),
        Err(StoreError::Busy)
    ));
    gate.release();
    assert!(commit.join().unwrap().is_ok());
    fixture
        .store
        .abort_upload(&fixture.credential, &fixture.upload_id)
        .unwrap();
    assert_eq!(
        fixture
            .store
            .get_upload_state(&fixture.credential, &fixture.upload_id)
            .unwrap()
            .state,
        UploadState::Committed
    );
    assert!(matches!(
        fixture
            .store
            .commit_upload(&fixture.credential, &fixture.upload_id),
        Err(StoreError::Conflict)
    ));
    assert_new_bytes(&fixture);
}
