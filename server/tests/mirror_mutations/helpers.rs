use drip_core::SourcePath;
use drip_core::protocol::{
    DirectoryCreateRequest, Entry, EntryDeleteRequest, EntryDeleteResponse, EntryPrecondition,
};
use drip_server::StoreResult;

use crate::common::Fixture;

pub(crate) fn create_directory(
    fixture: &Fixture,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source_path: SourcePath,
) {
    fixture
        .store
        .create_directory(
            credential,
            DirectoryCreateRequest {
                selection_id: selection_id.to_owned(),
                generation,
                source_path,
                expected_entry: None,
            },
        )
        .unwrap();
}

pub(crate) fn delete(
    fixture: &Fixture,
    credential: &str,
    selection_id: &str,
    generation: u64,
    source_path: &SourcePath,
    expected: EntryPrecondition,
) -> StoreResult<EntryDeleteResponse> {
    fixture.store.delete_entry(
        credential,
        EntryDeleteRequest {
            selection_id: selection_id.to_owned(),
            generation,
            source_path: source_path.clone(),
            expected_entry: expected,
        },
    )
}

pub(crate) fn entry(entries: &[Entry], source: &SourcePath) -> EntryPrecondition {
    let entry = entries
        .iter()
        .find(|entry| &entry.source_path == source)
        .unwrap();
    EntryPrecondition {
        kind: entry.kind,
        revision: entry.revision,
    }
}

pub(crate) fn child(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
