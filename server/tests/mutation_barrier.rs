mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use common::Fixture;
use drip_core::{
    SourcePath,
    protocol::{Selection, SelectionUpdate},
};
use drip_server::StoreResult;

#[test]
fn revocation_waits_for_older_authorized_mutation() {
    let fixture = Fixture::new();
    let device = enroll(&fixture, "Race");
    let credential = device.device_credential.expose_secret().to_owned();
    let selection_id = "a".repeat(32);
    fixture
        .store
        .update_selections(
            &credential,
            SelectionUpdate {
                expected_generation: 0,
                selections: vec![Selection {
                    id: selection_id.clone(),
                    source: SourcePath::new('C', vec!["file.txt".to_owned()]).unwrap(),
                    active: true,
                }],
            },
        )
        .unwrap();
    let store = fixture.store.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mutation = thread::spawn(move || {
        store.with_authorized_selection_mutation(&credential, &selection_id, 1, |_, _, _, _| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
    });
    entered_rx.recv().unwrap();
    let store = fixture.store.clone();
    let pc_id = device.pc_id.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let revoked = Arc::new(AtomicBool::new(false));
    let completed = Arc::clone(&revoked);
    let revoker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = store.revoke_device(&pc_id);
        completed.store(result.is_ok(), Ordering::SeqCst);
        result
    });
    started_rx.recv().unwrap();
    thread::sleep(Duration::from_millis(30));
    assert!(!revoked.load(Ordering::SeqCst));
    release_tx.send(()).unwrap();
    let mutation_result: StoreResult<()> = mutation.join().unwrap();
    let revoke_result = revoker.join().unwrap();
    assert!(mutation_result.is_ok() && revoke_result.is_ok());
}

#[cfg(unix)]
#[test]
fn device_root_symlink_cannot_cross_namespace() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let first = enroll(&fixture, "One");
    let second = enroll(&fixture, "Two");
    let selection_id = "a".repeat(32);
    fixture
        .store
        .update_selections(
            first.device_credential.expose_secret(),
            SelectionUpdate {
                expected_generation: 0,
                selections: vec![Selection {
                    id: selection_id.clone(),
                    source: SourcePath::new('C', vec!["file.txt".to_owned()]).unwrap(),
                    active: true,
                }],
            },
        )
        .unwrap();
    let first_path = fixture._root.join("mirror").join(&first.mirror_directory);
    let second_path = fixture._root.join("mirror").join(&second.mirror_directory);
    std::fs::remove_dir(&first_path).unwrap();
    symlink(&second_path, &first_path).unwrap();
    assert!(
        fixture
            .store
            .with_authorized_selection_mutation(
                first.device_credential.expose_secret(),
                &selection_id,
                1,
                |_, _, _, _| Ok(()),
            )
            .is_err()
    );
}

fn enroll(fixture: &Fixture, label: &str) -> drip_core::protocol::EnrollmentResponse {
    let token = fixture.store.create_enrollment_token().unwrap();
    fixture
        .store
        .enroll(drip_core::protocol::EnrollmentRequest {
            token: drip_core::SensitiveString::new(token),
            label: label.to_owned(),
        })
        .unwrap()
}
