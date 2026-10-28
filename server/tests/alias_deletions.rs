mod common;

use std::fs;

use common::{Fixture, enroll, mirror_path, select_kind};
#[cfg(not(windows))]
use drip_core::protocol::{Entry, EntryDeleteRequest, EntryPrecondition};
use drip_core::{SourcePath, protocol::EntryKind};
#[cfg(not(windows))]
use drip_server::StoreError;

#[test]
#[cfg(not(windows))]
fn file_root_retires_old_spelling_directory_without_authorizing_current_descendants() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "File alias cleanup");
    let other = enroll(&fixture.store, "Other PC");
    let credential = device.device_credential.expose_secret();
    let root = path(&["scope", "root.bin"]);
    let old = path(&["SCOPE", "ROOT.bin"]);
    let child = path(&["SCOPE", "ROOT.bin", "old.bin"]);
    let sibling = path(&["SCOPE", "outside.bin"]);
    let selection_id = "d".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root.clone(),
        EntryKind::File,
    );
    let current = mirror_path(&fixture, &device, &root);
    fs::create_dir_all(current.parent().unwrap()).unwrap();
    let old_root = mirror_path(&fixture, &device, &old);
    fs::create_dir_all(&old_root).unwrap();
    fs::write(mirror_path(&fixture, &device, &child), b"retired old tree").unwrap();
    fs::write(
        mirror_path(&fixture, &device, &sibling),
        b"unrelated sibling",
    )
    .unwrap();

    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.source_path == child)
        .unwrap();
    let request = deletion(&selection_id, generation, entry);
    assert!(matches!(
        fixture
            .store
            .delete_entry(other.device_credential.expose_secret(), request.clone()),
        Err(StoreError::NotFound)
    ));
    let current_descendant = EntryDeleteRequest {
        source_path: path(&["scope", "root.bin", "old.bin"]),
        ..request.clone()
    };
    assert!(matches!(
        fixture.store.delete_entry(credential, current_descendant),
        Err(StoreError::DisallowedNamespace)
    ));
    let outside = EntryDeleteRequest {
        source_path: sibling.clone(),
        ..request.clone()
    };
    assert!(matches!(
        fixture.store.delete_entry(credential, outside),
        Err(StoreError::DisallowedNamespace)
    ));
    assert!(matches!(
        fixture.store.delete_entry(credential, request.clone()),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &child)).unwrap(),
        b"retired old tree"
    );
    fs::write(&current, b"complete canonical file").unwrap();
    assert!(
        fixture
            .store
            .delete_entry(credential, request)
            .unwrap()
            .deleted
    );

    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let old_entry = page
        .entries
        .iter()
        .find(|entry| entry.source_path == old)
        .unwrap();
    let old_request = deletion(&selection_id, generation, old_entry);
    assert!(
        fixture
            .store
            .delete_entry(credential, old_request.clone())
            .unwrap()
            .deleted
    );
    assert!(!old_root.exists());
    assert_eq!(fs::read(&current).unwrap(), b"complete canonical file");
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &sibling)).unwrap(),
        b"unrelated sibling"
    );
    fs::remove_file(current).unwrap();
    assert!(
        !fixture
            .store
            .delete_entry(credential, old_request)
            .unwrap()
            .deleted
    );
}

#[test]
#[cfg(windows)]
fn selected_case_variant_resolves_to_one_windows_directory_entry() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Windows case lookup");
    let credential = device.device_credential.expose_secret();
    let selected = path(&["scope", "root.bin"]);
    let selection_id = "d".repeat(32);
    select_kind(
        &fixture.store,
        credential,
        &selection_id,
        selected.clone(),
        EntryKind::File,
    );

    let root = mirror_path(&fixture, &device, &selected);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("old.bin"), b"retained copy").unwrap();

    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    assert!(
        page.entries
            .iter()
            .any(|entry| { entry.source_path == path(&["scope", "root.bin", "old.bin"]) })
    );
    assert!(
        !page
            .entries
            .iter()
            .any(|entry| { entry.source_path == path(&["SCOPE", "ROOT.bin", "old.bin"]) })
    );
}

fn path(components: &[&str]) -> SourcePath {
    SourcePath::new(
        'C',
        components
            .iter()
            .map(|component| (*component).to_owned())
            .collect(),
    )
    .unwrap()
}

#[cfg(not(windows))]
fn deletion(selection_id: &str, generation: u64, entry: &Entry) -> EntryDeleteRequest {
    EntryDeleteRequest {
        selection_id: selection_id.to_owned(),
        generation,
        source_path: entry.source_path.clone(),
        expected_entry: EntryPrecondition {
            kind: entry.kind,
            revision: entry.revision,
        },
    }
}
