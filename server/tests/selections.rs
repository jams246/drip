mod common;

use common::Fixture;
use drip_core::{
    SourcePath,
    protocol::{EntryKind, Selection, SelectionUpdate},
};
use drip_server::StoreError;

#[test]
fn selection_generations_and_records_are_device_scoped() {
    let fixture = Fixture::new();
    let first = enroll(&fixture, "One");
    let second = enroll(&fixture, "Two");
    let initial = fixture
        .store
        .get_selections(first.device_credential.expose_secret())
        .unwrap();
    let updated = fixture
        .store
        .update_selections(
            first.device_credential.expose_secret(),
            SelectionUpdate {
                expected_generation: initial.generation,
                selections: vec![selection("a", "file.txt", true)],
            },
        )
        .unwrap();
    let other = fixture
        .store
        .get_selections(second.device_credential.expose_secret())
        .unwrap();
    assert_eq!(updated.generation, 1);
    assert_eq!(other.generation, 0);
    assert!(other.selections.is_empty());
}

#[test]
fn stale_selection_generation_conflicts() {
    let fixture = Fixture::new();
    let device = enroll(&fixture, "One");
    let credential = device.device_credential.expose_secret();
    fixture
        .store
        .update_selections(
            credential,
            SelectionUpdate {
                expected_generation: 0,
                selections: vec![selection("a", "file.txt", true)],
            },
        )
        .unwrap();
    assert!(matches!(
        fixture.store.update_selections(
            credential,
            SelectionUpdate {
                expected_generation: 0,
                selections: Vec::new(),
            }
        ),
        Err(StoreError::Conflict)
    ));
}

#[test]
fn removal_retains_inactive_record_and_advances_generation() {
    let fixture = Fixture::new();
    let device = enroll(&fixture, "One");
    let credential = device.device_credential.expose_secret();
    fixture
        .store
        .update_selections(
            credential,
            SelectionUpdate {
                expected_generation: 0,
                selections: vec![selection("a", "file.txt", true)],
            },
        )
        .unwrap();
    let removed = fixture
        .store
        .update_selections(
            credential,
            SelectionUpdate {
                expected_generation: 1,
                selections: Vec::new(),
            },
        )
        .unwrap();
    let current = fixture.store.get_selections(credential).unwrap();
    assert_eq!(removed.generation, 2);
    assert_eq!(current.selections, vec![selection("a", "file.txt", false)]);
}

#[test]
fn selection_noop_still_advances_generation() {
    let fixture = Fixture::new();
    let device = enroll(&fixture, "One");
    let result = fixture
        .store
        .update_selections(
            device.device_credential.expose_secret(),
            SelectionUpdate {
                expected_generation: 0,
                selections: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(result.generation, 1);
}

fn selection(id: &str, name: &str, active: bool) -> Selection {
    Selection {
        id: format!("{}{}", "a".repeat(32 - id.len()), id),
        source: SourcePath::new('C', vec![name.to_owned()]).unwrap(),
        root_kind: EntryKind::File,
        active,
    }
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
