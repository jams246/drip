mod common;
#[path = "mirror_exchanges/helpers.rs"]
mod helpers;

use std::fs;

use common::{Fixture, enroll, mirror_path, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::{DirectoryCreateRequest, EntryKind};
use helpers::{child, entry_precondition};

#[test]
fn directory_revision_changes_when_a_child_is_added() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Directory revision");
    let credential = device.device_credential.expose_secret();
    let root = SourcePath::new('C', vec!["tree".to_owned()]).unwrap();
    let selection_id = "d".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::Directory,
    );
    fixture
        .store
        .create_directory(
            credential,
            DirectoryCreateRequest {
                selection_id: selection_id.clone(),
                generation,
                source_path: root.clone(),
                expected_entry: None,
            },
        )
        .unwrap();
    let first = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let initial_revision = entry_precondition(&first.entries, &root).revision;
    fs::write(
        mirror_path(&fixture, &device, &child(&root, "child.txt")),
        b"child",
    )
    .unwrap();
    let second = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    assert!(entry_precondition(&second.entries, &root).revision > initial_revision);
}
