use drip_core::{
    SourcePath,
    protocol::{Entry, EntryKind, EntryPage},
};

use crate::{
    Store, StoreError, StoreResult, entry_files, selections, upload_files, upload_metadata,
    upload_store::authorized_active_selection,
};

mod scan;

impl Store {
    /// Lists actual files and directories under one active selection.
    pub fn list_entries(
        &self,
        credential: &str,
        selection_id: &str,
        cursor: Option<&str>,
    ) -> StoreResult<EntryPage> {
        self.list_entries_at_generation(credential, selection_id, None, cursor)
    }

    /// Lists one generation-bound page of actual mirror metadata.
    pub fn list_entries_at_generation(
        &self,
        credential: &str,
        selection_id: &str,
        expected_generation: Option<u64>,
        cursor: Option<&str>,
    ) -> StoreResult<EntryPage> {
        let (device, selection, generation) =
            authorized_active_selection(self, credential, selection_id)?;
        if expected_generation.is_some_and(|expected| expected != generation) {
            return Err(StoreError::Conflict);
        }
        if cursor == Some("end") {
            return Ok(empty_page(selection.id));
        }
        let after = cursor.map(decode_cursor).transpose()?;
        if after
            .as_ref()
            .is_some_and(|source| !selections::selection_contains_path(&selection, source))
        {
            return Err(StoreError::InvalidInput);
        }
        let mirror = self.open_device_directory(&device)?;
        let scan = scan::page(&mirror, &selection.source, after.as_ref())?;
        let has_more = scan.has_more;
        let last_path = scan.entries.last().map(|entry| entry.source_path.clone());
        let entries = self.with_authorized_selection_mutation(
            credential,
            selection_id,
            generation,
            |transaction, current_device, current_selection, directory| {
                if current_selection.source != selection.source
                    || current_selection.root_kind != selection.root_kind
                {
                    return Err(StoreError::Conflict);
                }
                scan.entries
                    .into_iter()
                    .map(|item| {
                        let (kind, size, digest, last_write, revision) = match item.snapshot {
                            entry_files::EntrySnapshot::File(snapshot) => {
                                upload_files::check_snapshot(
                                    directory,
                                    &item.source_path,
                                    Some(&snapshot),
                                )?;
                                let revision = upload_metadata::record_snapshot(
                                    transaction,
                                    &current_device.pc_id,
                                    &item.source_path,
                                    Some(&snapshot),
                                )?
                                .ok_or(StoreError::Conflict)?;
                                (
                                    EntryKind::File,
                                    snapshot.size,
                                    Some(snapshot.digest),
                                    Some(snapshot.last_write),
                                    revision,
                                )
                            }
                            entry_files::EntrySnapshot::Directory(snapshot) => {
                                entry_files::check_directory(
                                    directory,
                                    &item.source_path,
                                    snapshot,
                                )?;
                                let revision = upload_metadata::record_directory_snapshot(
                                    transaction,
                                    &current_device.pc_id,
                                    &item.source_path,
                                    snapshot,
                                )?;
                                (EntryKind::Directory, 0, None, None, revision)
                            }
                        };
                        Ok(Entry {
                            source_path: item.source_path,
                            kind,
                            size,
                            digest,
                            revision,
                            last_write,
                        })
                    })
                    .collect::<StoreResult<Vec<_>>>()
            },
        )?;
        let next_cursor = if has_more {
            Some(encode_cursor(last_path.ok_or(StoreError::Conflict)?)?)
        } else {
            None
        };
        Ok(EntryPage {
            selection_id: selection.id,
            entries,
            next_cursor,
        })
    }
}

fn encode_cursor(source: SourcePath) -> StoreResult<String> {
    let bytes = serde_json::to_vec(&source).map_err(StoreError::Serialization)?;
    let mut cursor = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        cursor.push(char::from(HEX[usize::from(byte >> 4)]));
        cursor.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    if cursor.len() > 4096 {
        return Err(StoreError::UnsupportedEntry);
    }
    Ok(cursor)
}

fn validate_cursor_path(source: &SourcePath) -> StoreResult<()> {
    let bytes = serde_json::to_vec(source).map_err(StoreError::Serialization)?;
    if bytes.len() * 2 > 4096 {
        Err(StoreError::UnsupportedEntry)
    } else {
        Ok(())
    }
}

fn decode_cursor(cursor: &str) -> StoreResult<SourcePath> {
    if cursor.is_empty() || cursor.len() > 4096 || !cursor.len().is_multiple_of(2) {
        return Err(StoreError::InvalidInput);
    }
    let mut bytes = Vec::with_capacity(cursor.len() / 2);
    for pair in cursor.as_bytes().as_chunks::<2>().0 {
        let high = hex_value(pair[0]).ok_or(StoreError::InvalidInput)?;
        let low = hex_value(pair[1]).ok_or(StoreError::InvalidInput)?;
        bytes.push((high << 4) | low);
    }
    serde_json::from_slice(&bytes).map_err(|_| StoreError::InvalidInput)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn empty_page(selection_id: String) -> EntryPage {
    EntryPage {
        selection_id,
        entries: Vec::new(),
        next_cursor: None,
    }
}
