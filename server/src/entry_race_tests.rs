use std::thread::JoinHandle;

use drip_core::protocol::{EntryDeleteRequest, EntryPrecondition, SelectionUpdate};

use crate::{
    StoreError, StoreResult,
    upload_race_test_common::{
        self as common, CheckpointGate, Fixture, assert_cli_busy, assert_cli_success,
        assert_old_bytes,
    },
};

#[tokio::test]
async fn selection_removal_before_writer_lock_fences_delete() {
    let fixture = Fixture::new().await;
    let (mut gate, deletion, request) = start_delete(&fixture, "before_writer_lock");
    assert_eq!(fixture.remove_selection().unwrap(), fixture.generation + 1);
    gate.release();
    assert!(matches!(
        deletion.join().unwrap(),
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        fixture.store.delete_entry(&fixture.credential, request),
        Err(StoreError::Conflict)
    ));
    assert_old_bytes(&fixture);
}

#[tokio::test]
async fn pause_generation_before_writer_lock_fences_delete() {
    let fixture = Fixture::new().await;
    let (mut gate, deletion, request) = start_delete(&fixture, "before_writer_lock");
    assert_eq!(
        advance_generation(&fixture).unwrap(),
        fixture.generation + 1
    );
    gate.release();
    assert!(matches!(
        deletion.join().unwrap(),
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        fixture.store.delete_entry(&fixture.credential, request),
        Err(StoreError::Conflict)
    ));
    assert_old_bytes(&fixture);
}

#[tokio::test]
async fn credential_revocation_before_writer_lock_fences_delete() {
    let fixture = Fixture::new().await;
    let (mut gate, deletion, request) = start_delete(&fixture, "before_writer_lock");
    assert!(fixture.run_revoke_cli().status.success());
    gate.release();
    assert!(matches!(
        deletion.join().unwrap(),
        Err(StoreError::InvalidCredential)
    ));
    assert!(matches!(
        fixture.store.delete_entry(&fixture.credential, request),
        Err(StoreError::InvalidCredential)
    ));
    assert_old_bytes(&fixture);
}

#[tokio::test]
async fn selection_removal_waits_for_delete_inside_writer_barrier() {
    let fixture = Fixture::new().await;
    let (mut gate, deletion, request) = start_delete(&fixture, "before_entry_delete");
    assert!(matches!(fixture.remove_selection(), Err(StoreError::Busy)));
    gate.release();
    assert!(deletion.join().unwrap().unwrap().deleted);
    assert_eq!(fixture.remove_selection().unwrap(), fixture.generation + 1);
    assert!(matches!(
        fixture.store.delete_entry(&fixture.credential, request),
        Err(StoreError::Conflict)
    ));
    assert_entry_absent(&fixture);
}

#[tokio::test]
async fn pause_generation_waits_for_delete_inside_writer_barrier() {
    let fixture = Fixture::new().await;
    let (mut gate, deletion, request) = start_delete(&fixture, "before_entry_delete");
    assert!(matches!(
        advance_generation(&fixture),
        Err(StoreError::Busy)
    ));
    gate.release();
    assert!(deletion.join().unwrap().unwrap().deleted);
    assert_eq!(
        advance_generation(&fixture).unwrap(),
        fixture.generation + 1
    );
    assert!(matches!(
        fixture.store.delete_entry(&fixture.credential, request),
        Err(StoreError::Conflict)
    ));
    assert_entry_absent(&fixture);
}

#[tokio::test]
async fn credential_revocation_waits_for_delete_inside_writer_barrier() {
    let fixture = Fixture::new().await;
    let (mut gate, deletion, request) = start_delete(&fixture, "before_entry_delete");
    assert_cli_busy(&fixture.run_revoke_cli());
    gate.release();
    assert!(deletion.join().unwrap().unwrap().deleted);
    assert_cli_success(&fixture.run_revoke_cli());
    assert!(matches!(
        fixture.store.delete_entry(&fixture.credential, request),
        Err(StoreError::InvalidCredential)
    ));
    assert_entry_absent(&fixture);
}

#[tokio::test]
async fn pause_generation_before_writer_lock_fences_staged_upload() {
    let fixture = Fixture::new().await;
    let (mut gate, commit) = fixture.start_commit("before_writer_lock");
    assert_eq!(
        advance_generation(&fixture).unwrap(),
        fixture.generation + 1
    );
    gate.release();
    assert!(matches!(commit.join().unwrap(), Err(StoreError::Conflict)));
    assert_old_bytes(&fixture);
}

fn start_delete(
    fixture: &Fixture,
    checkpoint: &'static str,
) -> (
    CheckpointGate,
    JoinHandle<StoreResult<drip_core::protocol::EntryDeleteResponse>>,
    EntryDeleteRequest,
) {
    let selection_id = "a".repeat(32);
    let entry = fixture
        .store
        .list_entries(&fixture.credential, &selection_id, None)
        .unwrap()
        .entries
        .into_iter()
        .find(|entry| entry.source_path == fixture.source)
        .unwrap();
    let request = EntryDeleteRequest {
        selection_id,
        generation: fixture.generation,
        source_path: fixture.source.clone(),
        expected_entry: EntryPrecondition {
            kind: entry.kind,
            revision: entry.revision,
        },
    };
    let stale_request = request.clone();
    let gate = CheckpointGate::new(checkpoint);
    let store = fixture.store.clone();
    let credential = fixture.credential.clone();
    let deletion = gate.spawn(move || store.delete_entry(&credential, request));
    gate.wait_until_entered();
    (gate, deletion, stale_request)
}

fn advance_generation(fixture: &Fixture) -> StoreResult<u64> {
    let selections = fixture
        .store
        .get_selections(&fixture.credential)?
        .selections;
    fixture
        .store
        .update_selections(
            &fixture.credential,
            SelectionUpdate {
                expected_generation: fixture.generation,
                selections,
            },
        )
        .map(|response| response.generation)
}

fn assert_entry_absent(fixture: &Fixture) {
    let path = common::mirror_path(
        &fixture.mirror_dir,
        &fixture.device_directory,
        &fixture.source,
    );
    assert!(!path.exists());
}
