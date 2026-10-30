mod common;

use common::{Fixture, enroll};
use drip_core::{
    SourcePath,
    protocol::{EntryKind, Selection, SelectionUpdate},
};
use drip_server::StoreError;

#[test]
fn overdeep_root_is_rejected_at_selection_time() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Inventory depth limit");
    let credential = device.device_credential.expose_secret();
    let path = SourcePath::new('C', vec!["x".into(); 257]).unwrap();
    let update = SelectionUpdate {
        expected_generation: 0,
        selections: vec![Selection {
            id: "a".repeat(32),
            source: path,
            root_kind: EntryKind::Directory,
            active: true,
        }],
    };
    assert!(matches!(
        fixture.store.update_selections(credential, update),
        Err(StoreError::InvalidInput)
    ));
    assert_eq!(
        fixture.store.get_selections(credential).unwrap().generation,
        0
    );
}

#[cfg(unix)]
#[test]
fn excessive_case_aliases_return_no_partial_inventory() {
    use common::select_kind;
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Inventory alias limit");
    let credential = device.device_credential.expose_secret();
    let id = "b".repeat(32);
    let source = SourcePath::new('C', vec!["abcdefghi".into()]).unwrap();
    select_kind(
        &fixture.store,
        credential,
        &id,
        source,
        EntryKind::Directory,
    );
    let drive = fixture
        ._root
        .join("mirror")
        .join(&device.mirror_directory)
        .join("C");
    std::fs::create_dir(&drive).unwrap();
    for variant in 0..257 {
        let name: String = "abcdefghi"
            .chars()
            .enumerate()
            .map(|(index, letter)| {
                if variant & (1 << index) == 0 {
                    letter
                } else {
                    letter.to_ascii_uppercase()
                }
            })
            .collect();
        std::fs::create_dir(drive.join(name)).unwrap();
    }
    assert!(matches!(
        fixture.store.list_entries(credential, &id, None),
        Err(StoreError::UnsupportedEntry)
    ));
    assert_eq!(std::fs::read_dir(drive).unwrap().count(), 257);
}
