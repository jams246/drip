mod common;

use std::fs;

use common::{Fixture, enroll, mirror_path, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::{
    DirectoryCreateRequest, EntryDeleteRequest, EntryKind, EntryPrecondition,
};
use drip_server::StoreError;

#[test]
fn directory_authority_covers_case_alias_descendants_only_for_same_pc() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Alias cleanup");
    let credential = device.device_credential.expose_secret();
    let active_root = source(&["workspace", "caseauthority"]);
    let selection_id = "a".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        active_root,
        EntryKind::Directory,
    );
    let alias_child = source(&["Workspace", "CaseAuthority", "child"]);
    fs::create_dir_all(mirror_path(&fixture, &device, &alias_child)).unwrap();
    let page = fixture
        .store
        .list_entries_at_generation(credential, &selection_id, Some(generation), None)
        .unwrap();
    let child_entry = page
        .entries
        .iter()
        .find(|entry| entry.source_path == alias_child)
        .unwrap();
    let deleted = fixture
        .store
        .delete_entry(
            credential,
            EntryDeleteRequest {
                selection_id: selection_id.clone(),
                generation,
                source_path: alias_child,
                expected_entry: EntryPrecondition {
                    kind: EntryKind::Directory,
                    revision: child_entry.revision,
                },
            },
        )
        .unwrap();
    assert!(deleted.deleted);

    let sibling_child = source(&["workspace", "caseauthority-sibling", "child"]);
    assert!(matches!(
        fixture.store.delete_entry(
            credential,
            EntryDeleteRequest {
                selection_id: selection_id.clone(),
                generation,
                source_path: sibling_child,
                expected_entry: EntryPrecondition {
                    kind: EntryKind::Directory,
                    revision: 1,
                },
            },
        ),
        Err(StoreError::DisallowedNamespace)
    ));

    let other_device = enroll(&fixture.store, "Foreign alias");
    assert!(matches!(
        fixture.store.delete_entry(
            other_device.device_credential.expose_secret(),
            EntryDeleteRequest {
                selection_id,
                generation: 0,
                source_path: source(&["Workspace", "CaseAuthority", "child"]),
                expected_entry: EntryPrecondition {
                    kind: EntryKind::Directory,
                    revision: 1,
                },
            },
        ),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn file_selection_still_rejects_descendant_mutations() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "File root boundary");
    let credential = device.device_credential.expose_secret();
    let root = source(&["workspace", "file.bin"]);
    let selection_id = "f".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::File,
    );
    let result = fixture.store.create_directory(
        credential,
        DirectoryCreateRequest {
            selection_id,
            generation,
            source_path: child(&root, "descendant"),
            expected_entry: None,
        },
    );
    assert!(matches!(result, Err(StoreError::DisallowedNamespace)));
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

fn child(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
