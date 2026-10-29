mod common;

use std::{
    path::PathBuf,
    sync::{
        Arc, Barrier,
        atomic::{AtomicI64, Ordering},
    },
    thread,
};

use common::{Fixture, unique_fixture_directory};
use drip_core::{SensitiveString, protocol::EnrollmentRequest};
use drip_server::{Store, StoreError};

#[test]
fn enrollment_token_is_valid_until_exactly_fifteen_minutes() {
    let fixture = ClockFixture::new();
    let token = fixture.store.create_enrollment_token().unwrap();
    fixture.clock.store(1_899, Ordering::SeqCst);
    assert!(fixture.store.enroll(request(token, "Boundary")).is_ok());
}

#[test]
fn enrollment_rejects_token_at_fifteen_minute_boundary() {
    let fixture = ClockFixture::new();
    let token = fixture.store.create_enrollment_token().unwrap();
    fixture.clock.store(1_900, Ordering::SeqCst);
    assert!(matches!(
        fixture.store.enroll(request(token, "Boundary")),
        Err(StoreError::InvalidCredential)
    ));
}

#[test]
fn concurrent_enrollment_consumes_token_once_across_connections() {
    let fixture = Fixture::new();
    let token = fixture.store.create_enrollment_token().unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|_| {
            let store = fixture.store.clone();
            let barrier = Arc::clone(&barrier);
            let token = token.clone();
            thread::spawn(move || {
                barrier.wait();
                store.enroll(request(token, "Concurrent"))
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
}

#[test]
fn revocation_blocks_authentication_and_appears_in_device_list() {
    let fixture = Fixture::new();
    let device = enroll(&fixture, "Studio PC");
    fixture.store.revoke_device(&device.pc_id).unwrap();
    assert!(matches!(
        fixture
            .store
            .get_selections(device.device_credential.expose_secret()),
        Err(StoreError::InvalidCredential)
    ));
    let record = fixture.store.list_devices().unwrap().remove(0);
    assert!(!record.active);
    assert_eq!(record.label, "Studio PC");
}

#[test]
fn matching_labels_get_distinct_mirror_directories() {
    let fixture = Fixture::new();
    let first = enroll(&fixture, "Same label");
    let second = enroll(&fixture, "Same label");
    assert_ne!(first.pc_id, second.pc_id);
    assert_ne!(first.mirror_directory, second.mirror_directory);
}

fn enroll(fixture: &Fixture, label: &str) -> drip_core::protocol::EnrollmentResponse {
    let token = fixture.store.create_enrollment_token().unwrap();
    fixture.store.enroll(request(token, label)).unwrap()
}

fn request(token: String, label: &str) -> EnrollmentRequest {
    EnrollmentRequest {
        token: SensitiveString::new(token),
        label: label.to_owned(),
    }
}

struct ClockFixture {
    root: PathBuf,
    store: Store,
    clock: Arc<AtomicI64>,
}

impl ClockFixture {
    fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-02/server-tests");
        let root = unique_fixture_directory(&parent, "clock");
        let clock = Arc::new(AtomicI64::new(1_000));
        let test_clock = Arc::clone(&clock);
        let store = Store::open_with_clock(root.join("state"), root.join("mirror"), move || {
            Ok(test_clock.load(Ordering::SeqCst))
        })
        .unwrap();
        Self { root, store, clock }
    }
}

impl Drop for ClockFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
