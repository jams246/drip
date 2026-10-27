use cap_fs_ext::DirExt;
use drip_core::protocol::{DirectoryCreateRequest, DirectoryCreateResponse, Entry, EntryKind};

use crate::{
    Store, StoreError, StoreResult, database,
    entry_files::{self, EntrySnapshot},
    selections, upload_files, upload_metadata,
    upload_store::authorized_selection,
};

impl Store {
    /// Creates one directory under the active selection's mutation barrier.
    pub fn create_directory(
        &self,
        credential: &str,
        request: DirectoryCreateRequest,
    ) -> StoreResult<DirectoryCreateResponse> {
        let (device, selection) =
            authorized_selection(self, credential, &request.selection_id, request.generation)?;
        if !selections::selection_contains_path(&selection, &request.source_path) {
            return Err(StoreError::DisallowedNamespace);
        }
        if selection.root_kind == EntryKind::File
            && request.source_path == selection.source
            && request.expected_entry.is_none()
        {
            return Err(StoreError::DisallowedNamespace);
        }
        if request.expected_entry.is_some() {
            return Err(StoreError::Conflict);
        }
        let mirror = self.open_device_directory(&device)?;
        let existing = entry_files::read_entry(&mirror, &request.source_path)?;
        if existing
            .as_ref()
            .is_some_and(|entry| entry.kind() != EntryKind::Directory)
        {
            return Err(StoreError::Conflict);
        }
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
                let snapshot = match existing {
                    Some(EntrySnapshot::Directory(snapshot)) => {
                        entry_files::check_directory(directory, &request.source_path, snapshot)?;
                        snapshot
                    }
                    None => {
                        entry_files::check_absent(directory, &request.source_path)?;
                        let (parent, name) =
                            entry_files::create_parent(directory, &request.source_path)?;
                        upload_files::checkpoint("before_directory_create");
                        match parent.create_dir(&name) {
                            Ok(()) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                                return Err(StoreError::Conflict);
                            }
                            Err(error) => return Err(database::io_error(error)),
                        }
                        let child = parent
                            .open_dir_nofollow(&name)
                            .map_err(database::io_error)?;
                        upload_files::fsync_directory(&child)?;
                        upload_files::fsync_directory(&parent)?;
                        entry_files::snapshot_directory(&child)?
                    }
                    Some(EntrySnapshot::File(_)) => return Err(StoreError::Conflict),
                };
                let revision = upload_metadata::record_directory_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &request.source_path,
                    snapshot,
                )?;
                Ok(DirectoryCreateResponse {
                    entry: Entry {
                        source_path: request.source_path.clone(),
                        kind: EntryKind::Directory,
                        size: 0,
                        digest: None,
                        revision,
                        last_write: None,
                    },
                })
            },
        )
    }
}
