use cap_fs_ext::DirExt;
use drip_core::protocol::{DirectoryCreateRequest, DirectoryCreateResponse, Entry, EntryKind};

use crate::{
    Store, StoreError, StoreResult, database,
    entry_files::{self, EntrySnapshot},
    selections, upload_exchange, upload_files, upload_metadata,
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
        if selection.root_kind == EntryKind::File && request.source_path == selection.source {
            return Err(StoreError::DisallowedNamespace);
        }
        let mirror = self.open_device_directory(&device)?;
        let existing = entry_files::read_entry(&mirror, &request.source_path)?;
        let (staging, stage_name, staged_snapshot) = match existing.as_ref() {
            Some(EntrySnapshot::File(_)) => {
                if request
                    .expected_entry
                    .is_none_or(|entry| entry.kind != EntryKind::File)
                {
                    return Err(StoreError::Conflict);
                }
                if !self.exchange_supported {
                    return Err(StoreError::UnsupportedExchange);
                }
                let staging = self.uploads.staging_dir()?;
                let stage_name = self
                    .uploads
                    .create_directory_stage(&crate::upload_store::random_upload_id()?)?;
                let staged = match staging.open_dir_nofollow(&stage_name) {
                    Ok(staged) => staged,
                    Err(error) => {
                        let _ = self.uploads.remove_directory_stage(&stage_name);
                        return Err(crate::database::io_error(error));
                    }
                };
                let snapshot = match entry_files::snapshot_directory(&staged) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        let _ = self.uploads.remove_directory_stage(&stage_name);
                        return Err(error);
                    }
                };
                (Some(staging), Some(stage_name), Some(snapshot))
            }
            Some(EntrySnapshot::Directory(_)) if request.expected_entry.is_some() => {
                return Err(StoreError::Conflict);
            }
            None if request.expected_entry.is_some() => return Err(StoreError::Conflict),
            _ => (None, None, None),
        };
        let mut retired_kind = None;
        let result = self.with_authorized_selection_mutation(
            credential,
            &request.selection_id,
            request.generation,
            |transaction, current_device, current_selection, directory| {
                if current_selection.source != selection.source
                    || current_selection.root_kind != selection.root_kind
                {
                    return Err(StoreError::Conflict);
                }
                let snapshot = match existing.as_ref() {
                    Some(EntrySnapshot::Directory(snapshot)) => {
                        entry_files::check_directory(directory, &request.source_path, *snapshot)?;
                        *snapshot
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
                    Some(EntrySnapshot::File(file_snapshot)) => {
                        let expected = request.expected_entry.ok_or(StoreError::Conflict)?;
                        entry_files::check_entry(
                            directory,
                            &request.source_path,
                            existing.as_ref(),
                        )?;
                        let revision = upload_metadata::record_snapshot(
                            transaction,
                            &current_device.pc_id,
                            &request.source_path,
                            Some(file_snapshot),
                        )?
                        .ok_or(StoreError::Conflict)?;
                        if revision != expected.revision {
                            return Err(StoreError::Conflict);
                        }
                        let (parent, filename) =
                            entry_files::open_parent(directory, &request.source_path)?
                                .ok_or(StoreError::Conflict)?;
                        let staging = staging.as_ref().ok_or(StoreError::Conflict)?;
                        let stage_name = stage_name.as_deref().ok_or(StoreError::Conflict)?;
                        let staged_snapshot = staged_snapshot.ok_or(StoreError::Conflict)?;
                        let paths = upload_exchange::ExchangePaths {
                            staging,
                            stage_name,
                            parent: &parent,
                            filename: &filename,
                        };
                        let target = upload_exchange::SelectedEntry {
                            root: directory,
                            source: &request.source_path,
                        };
                        if let Err(failure) = upload_exchange::replace_file_with_directory(
                            &paths,
                            &target,
                            file_snapshot,
                            staged_snapshot,
                        ) {
                            if failure.renamed {
                                retired_kind = Some(EntryKind::File);
                            }
                            return Err(failure.error);
                        }
                        retired_kind = Some(EntryKind::File);
                        let target = parent
                            .open_dir_nofollow(&filename)
                            .map_err(crate::database::io_error)?;
                        entry_files::snapshot_directory(&target)?
                    }
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
        );
        if let Some(stage_name) = stage_name {
            if retired_kind.is_some() {
                upload_files::checkpoint("before_retired_cleanup");
            }
            let cleanup = match retired_kind {
                Some(kind) => self.uploads.remove_retired_stage(&stage_name, kind),
                None => self.uploads.remove_directory_stage(&stage_name),
            };
            match result {
                Ok(response) => {
                    cleanup?;
                    return Ok(response);
                }
                Err(error) => {
                    let _ = cleanup;
                    return Err(error);
                }
            }
        }
        result
    }
}
