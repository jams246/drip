#![allow(dead_code)]

use drip_core::protocol::{Entry, EntryPrecondition, UploadPrepareRequest};
use drip_core::{Blake3Digest, SourcePath, Timestamp};

pub(crate) fn selection(
    selection_id: &str,
    generation: u64,
    source_path: SourcePath,
    bytes: &[u8],
    expected_entry: Option<EntryPrecondition>,
) -> UploadPrepareRequest {
    UploadPrepareRequest {
        selection_id: selection_id.to_owned(),
        generation,
        source_path,
        expected_size: bytes.len() as u64,
        digest: Blake3Digest::new(*blake3::hash(bytes).as_bytes()),
        last_write: timestamp(),
        expected_entry,
    }
}

pub(crate) fn entry_precondition(entries: &[Entry], source: &SourcePath) -> EntryPrecondition {
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

fn timestamp() -> Timestamp {
    Timestamp::new(1_700_000_000, 123_456_789).unwrap()
}
