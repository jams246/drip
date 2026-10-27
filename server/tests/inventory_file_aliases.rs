mod common;

use std::{collections::HashSet, fs};

use common::{Fixture, enroll, mirror_path, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::EntryKind;

#[test]
fn file_selection_inventory_pages_directory_and_file_case_aliases() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "File root aliases");
    let credential = device.device_credential.expose_secret();
    let selection = source(&["scope", "selected.bin"]);
    let selection_id = "f".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        selection.clone(),
        EntryKind::File,
    );
    let alias_file = source(&["SCOPE", "Selected.bin"]);
    let alias_path = mirror_path(&fixture, &device, &alias_file);
    fs::create_dir_all(alias_path.parent().unwrap()).unwrap();
    fs::write(&alias_path, b"old file alias").unwrap();

    let directory = mirror_path(&fixture, &device, &selection);
    fs::create_dir_all(&directory).unwrap();
    for index in 0..1100 {
        fs::write(directory.join(format!("item-{index:04}.bin")), b"dir").unwrap();
    }

    let mut expected = HashSet::from([alias_file, selection.clone()]);
    expected.extend((0..1100).map(|index| child(&selection, &format!("item-{index:04}.bin"))));
    let mut seen = HashSet::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = fixture
            .store
            .list_entries_at_generation(
                credential,
                &selection_id,
                Some(generation),
                cursor.as_deref(),
            )
            .unwrap();
        for entry in page.entries {
            assert!(seen.insert(entry.source_path));
        }
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(pages, 2);
    assert_eq!(seen, expected);
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
