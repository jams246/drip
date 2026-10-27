use drip_core::protocol::{EntryDeleteRequest, EntryDeleteResponse, EntryKind};

use crate::{
    Store, StoreError, StoreResult,
    entry_files::{self, EntrySnapshot},
    selections, upload_files, upload_metadata,
    upload_store::authorized_selection,
};

impl Store {
    /// Deletes one file or empty directory under the selection-generation barrier.
    pub fn delete_entry(
        &self,
        credential: &str,
        request: EntryDeleteRequest,
    ) -> StoreResult<EntryDeleteResponse> {
        let (device, selection) =
            authorized_selection(self, credential, &request.selection_id, request.generation)?;
        if !contains_delete_path(&selection, &request.source_path) {
            return Err(StoreError::DisallowedNamespace);
        }
        let mirror = self.open_device_directory(&device)?;
        let actual = entry_files::read_entry(&mirror, &request.source_path)?;
        let root_depth = selection.source.components().len();
        let replacement = if actual.is_some()
            && selection.root_kind == EntryKind::File
            && request.source_path.components()[..root_depth] != *selection.source.components()
        {
            match entry_files::read_entry(&mirror, &selection.source)? {
                Some(EntrySnapshot::File(snapshot)) => Some(snapshot),
                _ => return Err(StoreError::Conflict),
            }
        } else {
            None
        };
        if actual
            .as_ref()
            .is_some_and(|entry| entry.kind() != request.expected_entry.kind)
        {
            return Err(StoreError::Conflict);
        }
        upload_files::checkpoint("before_writer_lock");
        self.with_authorized_selection_mutation(
            credential,
            &request.selection_id,
            request.generation,
            |transaction, current_device, current_selection, directory| {
                if current_selection.source != selection.source
                    || current_selection.root_kind != selection.root_kind
                {
                    return Err(StoreError::Conflict);
                }
                if let Some(replacement) = &replacement {
                    upload_files::check_snapshot(directory, &selection.source, Some(replacement))?;
                }
                let Some(actual) = actual else {
                    entry_files::check_absent(directory, &request.source_path)?;
                    return Ok(EntryDeleteResponse { deleted: false });
                };
                let revision = match &actual {
                    EntrySnapshot::File(snapshot) => {
                        upload_files::check_snapshot(
                            directory,
                            &request.source_path,
                            Some(snapshot),
                        )?;
                        upload_metadata::record_snapshot(
                            transaction,
                            &current_device.pc_id,
                            &request.source_path,
                            Some(snapshot),
                        )?
                        .ok_or(StoreError::Conflict)?
                    }
                    EntrySnapshot::Directory(snapshot) => {
                        entry_files::check_directory(directory, &request.source_path, *snapshot)?;
                        upload_metadata::record_directory_snapshot(
                            transaction,
                            &current_device.pc_id,
                            &request.source_path,
                            *snapshot,
                        )?
                    }
                };
                if revision != request.expected_entry.revision {
                    return Err(StoreError::Conflict);
                }
                let (parent, name) = entry_files::open_parent(directory, &request.source_path)?
                    .ok_or(StoreError::Conflict)?;
                upload_files::checkpoint("before_entry_delete");
                match actual.kind() {
                    EntryKind::File => parent.remove_file(&name),
                    EntryKind::Directory => parent.remove_dir(&name),
                }
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                        StoreError::Conflict
                    } else {
                        crate::database::io_error(error)
                    }
                })?;
                upload_files::fsync_directory(&parent)?;
                upload_metadata::forget_entry(
                    transaction,
                    &current_device.pc_id,
                    &request.source_path,
                )?;
                Ok(EntryDeleteResponse { deleted: true })
            },
        )
    }
}

fn contains_delete_path(
    selection: &drip_core::protocol::Selection,
    source: &drip_core::SourcePath,
) -> bool {
    if selections::selection_contains_path(selection, source) {
        return true;
    }
    let root_depth = selection.source.components().len();
    // A verified file root can retire an old directory under a different canonical spelling.
    selection.root_kind == EntryKind::File
        && source.components().len() > root_depth
        && source.has_windows_prefix(&selection.source)
        && source.components()[..root_depth] != *selection.source.components()
}
