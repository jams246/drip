use drip_core::protocol::{Entry, EntryKind, EntryPage};

use crate::{
    Store, StoreError, StoreResult, entry_files, upload_files, upload_metadata,
    upload_store::authorized_active_selection,
};

mod cursor;
mod page;
mod roots;
mod scan;

use cursor::{CursorAnchor, decode as decode_cursor};

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
        if after.as_ref().is_some_and(|anchor| match anchor {
            CursorAnchor::Path(source) => !source.has_windows_prefix(&selection.source),
            CursorAnchor::Digest(_) => false,
        }) {
            return Err(StoreError::InvalidInput);
        }
        let mirror = self.open_device_directory(&device)?;
        let scan = scan::page(&mirror, &selection.source, after.as_ref())?;
        let has_more = scan.has_more;
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
        page::bounded(selection.id, entries, has_more)
    }
}

fn empty_page(selection_id: String) -> EntryPage {
    EntryPage {
        selection_id,
        entries: Vec::new(),
        next_cursor: None,
    }
}
