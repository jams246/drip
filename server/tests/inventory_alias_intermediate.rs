mod common;

use std::fs;

use common::{Fixture, enroll, mirror_path, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::EntryKind;
use drip_server::StoreError;

#[test]
fn inventory_fails_when_case_matching_intermediate_is_a_file() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Unsupported alias intermediate");
    let credential = device.device_credential.expose_secret();
    let selection = source(&["workspace", "caseauthority", "child"]);
    let selection_id = "a".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        selection,
        EntryKind::Directory,
    );
    let parent = mirror_path(&fixture, &device, &source(&["workspace"]));
    fs::create_dir_all(&parent).unwrap();
    fs::write(parent.join("CaseAuthority"), b"not a directory").unwrap();

    assert!(matches!(
        fixture
            .store
            .list_entries_at_generation(credential, &selection_id, Some(generation), None,),
        Err(StoreError::Conflict)
    ));
}

fn source(components: &[&str]) -> SourcePath {
    SourcePath::new(
        'C',
        components
            .iter()
            .map(|component| (*component).to_owned())
            .collect(),
    )
    .unwrap()
}
