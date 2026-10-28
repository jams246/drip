use super::*;

#[test]
fn inventory_resolves_case_variant_without_duplicate_windows_entries() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Windows case inventory");
    let credential = device.device_credential.expose_secret();
    let selected = source(&["Workspace", "CaseAuthority"]);
    let selection_id = "a".repeat(32);
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        selected.clone(),
        EntryKind::Directory,
    );
    create_directory(
        &fixture,
        credential,
        &selection_id,
        generation,
        selected.clone(),
    );
    fs::write(
        mirror_path(&fixture, &device, &selected).join("retained.bin"),
        b"retained copy",
    )
    .unwrap();

    let page = fixture
        .store
        .list_entries_at_generation(credential, &selection_id, Some(generation), None)
        .unwrap();
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].source_path, selected);
    assert_eq!(
        page.entries[1].source_path,
        source(&["Workspace", "CaseAuthority", "retained.bin"])
    );
}
