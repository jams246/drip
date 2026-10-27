mod common;

use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use common::{Fixture, enroll, request, select};
use drip_core::{SourcePath, Timestamp};
use drip_server::StoreError;

#[test]
fn one_active_per_device_and_eight_global_sessions_enforced() {
    let fixture = Fixture::new();
    let mut first_session = None;
    for index in 0..9 {
        let device = enroll(&fixture.store, &format!("Capacity {index}"));
        let credential = device.device_credential.expose_secret();
        let source = SourcePath::new('C', vec![format!("{index}.bin")]).unwrap();
        let selection_id = format!("{:032x}", index + 1);
        let generation = select(&fixture.store, credential, &selection_id, source.clone());
        let prepared = fixture.store.prepare_upload(
            credential,
            request(&selection_id, generation, source.clone(), b"", timestamp()),
        );
        if index < 8 {
            let prepared = prepared.unwrap();
            if index == 0 {
                first_session = Some((device, selection_id, generation, source, prepared));
            }
        } else {
            assert!(matches!(prepared, Err(StoreError::CapacityExhausted)));
        }
    }
    let (device, selection_id, generation, source, prepared) = first_session.unwrap();
    let credential = device.device_credential.expose_secret();
    assert!(matches!(
        fixture.store.prepare_upload(
            credential,
            request(&selection_id, generation, source, b"", timestamp()),
        ),
        Err(StoreError::CapacityExhausted)
    ));
    fixture
        .store
        .abort_upload(credential, &prepared.upload_id)
        .unwrap();
}

#[test]
fn idle_session_expires_at_thirty_minutes_and_frees_capacity() {
    let clock = Arc::new(AtomicI64::new(100));
    let fixture = Fixture::with_clock(Arc::clone(&clock));
    let device = enroll(&fixture.store, "Expiry");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["idle.bin".to_owned()]).unwrap();
    let selection_id = "1".repeat(32);
    let generation = select(&fixture.store, credential, &selection_id, source.clone());
    let prepared = fixture
        .store
        .prepare_upload(
            credential,
            request(&selection_id, generation, source.clone(), b"", timestamp()),
        )
        .unwrap();
    clock.store(100 + 30 * 60, Ordering::SeqCst);
    assert!(matches!(
        fixture
            .store
            .get_upload_state(credential, &prepared.upload_id),
        Err(StoreError::NotFound)
    ));
    assert!(
        !fixture
            ._root
            .join("mirror/.drip-staging")
            .join(format!("{}.part", prepared.upload_id))
            .exists()
    );
    assert!(
        fixture
            .store
            .prepare_upload(
                credential,
                request(&selection_id, generation, source, b"", timestamp()),
            )
            .is_ok()
    );
}

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}
