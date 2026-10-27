use std::sync::{Mutex, MutexGuard};

use drip_core::protocol::{UploadPrepareRequest, UploadPrepareResponse, UploadStateResponse};
use rusqlite::TransactionBehavior;

use crate::{
    Store, StoreError, StoreResult, auth, database, entry_files, selections, upload_metadata,
    upload_sessions::UploadSession,
};

impl Store {
    /// Creates an upload session for the exact path held by the active file selection.
    pub fn prepare_upload(
        &self,
        credential: &str,
        request: UploadPrepareRequest,
    ) -> StoreResult<UploadPrepareResponse> {
        let (device, selection) =
            authorized_selection(self, credential, &request.selection_id, request.generation)?;
        if request.source_path.components().is_empty()
            || !selections::selection_contains_path(&selection, &request.source_path)
        {
            return Err(StoreError::DisallowedNamespace);
        }
        if request
            .expected_entry
            .is_some_and(|entry| entry.kind != drip_core::protocol::EntryKind::Directory)
        {
            return Err(StoreError::Conflict);
        }
        let device_directory = self.open_device_directory(&device)?;
        let base_snapshot = entry_files::read_entry(&device_directory, &request.source_path)?;
        match (&base_snapshot, request.expected_entry) {
            (Some(entry), None) if entry.kind() == drip_core::protocol::EntryKind::Directory => {
                return Err(StoreError::Conflict);
            }
            (Some(entry), Some(expected)) if entry.kind() != expected.kind => {
                return Err(StoreError::Conflict);
            }
            (None, Some(_)) => return Err(StoreError::Conflict),
            _ => {}
        }
        if request.expected_entry.is_some() && !self.exchange_supported {
            return Err(StoreError::UnsupportedExchange);
        }
        let now = self.now()?;
        self.reap_uploads(now)?;
        let upload_id = random_upload_id()?;
        let stage_name = self.uploads.create_stage(&upload_id)?;
        let result = self.with_authorized_selection_mutation(
            credential,
            &request.selection_id,
            request.generation,
            |transaction, current_device, current_selection, directory| {
                if !selections::selection_contains_path(current_selection, &request.source_path) {
                    return Err(StoreError::DisallowedNamespace);
                }
                entry_files::check_entry(directory, &request.source_path, base_snapshot.as_ref())?;
                let base_revision = upload_metadata::record_entry_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &request.source_path,
                    base_snapshot.as_ref(),
                )?;
                if request
                    .expected_entry
                    .is_some_and(|expected| base_revision != Some(expected.revision))
                {
                    return Err(StoreError::Conflict);
                }
                self.uploads.reserve(UploadSession {
                    id: upload_id.clone(),
                    pc_id: current_device.pc_id.clone(),
                    selection_id: request.selection_id.clone(),
                    selection_root: current_selection.source.clone(),
                    generation: request.generation,
                    source_path: request.source_path.clone(),
                    expected_size: request.expected_size,
                    digest: request.digest,
                    last_write: request.last_write,
                    expected_entry: request.expected_entry,
                    base_revision,
                    base_snapshot: base_snapshot.clone(),
                    stage_name: stage_name.clone(),
                    state: drip_core::protocol::UploadState::Active,
                    last_progress: now,
                    receiving: false,
                    committing: false,
                    has_data: false,
                    delta_mode: false,
                    planning: false,
                })?;
                Ok(UploadPrepareResponse {
                    upload_id: upload_id.clone(),
                    base_revision,
                })
            },
        );
        if result.is_err() {
            self.uploads.remove(&upload_id);
            let _ = self.uploads.remove_stage(&stage_name);
        }
        result
    }

    /// Returns an authenticated upload session while it remains in daemon memory.
    pub fn get_upload_state(
        &self,
        credential: &str,
        upload_id: &str,
    ) -> StoreResult<UploadStateResponse> {
        let device = authenticated_device(self, credential)?;
        let now = self.now()?;
        self.reap_uploads(now)?;
        let session = self.uploads.get(upload_id, &device.pc_id, now)?;
        let session = lock(&session);
        Ok(UploadStateResponse {
            upload_id: session.id.clone(),
            state: session.state,
        })
    }

    /// Aborts an upload under the same writer barrier used by final publication.
    pub fn abort_upload(&self, credential: &str, upload_id: &str) -> StoreResult<()> {
        let now = self.now()?;
        self.with_authorized_mutation(credential, |_, device| {
            let session = self.uploads.get(upload_id, &device.pc_id, now)?;
            let mut session = lock(&session);
            if session.state == drip_core::protocol::UploadState::Active {
                self.uploads.remove_stage(&session.stage_name)?;
                session.state = drip_core::protocol::UploadState::Aborted;
                session.receiving = false;
                session.committing = false;
                session.has_data = false;
                session.last_progress = now;
            }
            Ok(())
        })?;
        self.delete_upload_plans(&[upload_id.to_owned()])
    }
}

pub(crate) fn authenticated_device(
    store: &Store,
    credential: &str,
) -> StoreResult<crate::DeviceIdentity> {
    let mut connection = store.connect()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(database::sql_error)?;
    let device = auth::authenticate_transaction(&transaction, credential)?;
    transaction.commit().map_err(database::sql_error)?;
    Ok(device)
}

pub(crate) fn authorized_selection(
    store: &Store,
    credential: &str,
    selection_id: &str,
    generation: u64,
) -> StoreResult<(crate::DeviceIdentity, drip_core::protocol::Selection)> {
    let mut connection = store.connect()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(database::sql_error)?;
    let device = auth::authenticate_transaction(&transaction, credential)?;
    let selection =
        selections::require_active_selection(&transaction, &device, selection_id, generation)?;
    transaction.commit().map_err(database::sql_error)?;
    Ok((device, selection))
}

pub(crate) fn authorized_active_selection(
    store: &Store,
    credential: &str,
    selection_id: &str,
) -> StoreResult<(crate::DeviceIdentity, drip_core::protocol::Selection, u64)> {
    if !auth::is_lower_hex(selection_id, 32) {
        return Err(StoreError::NotFound);
    }
    let mut connection = store.connect()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(database::sql_error)?;
    let device = auth::authenticate_transaction(&transaction, credential)?;
    let selections = selections::load_selections(&transaction, &device.pc_id)?;
    let generation = selections.generation;
    let selection = selections
        .selections
        .into_iter()
        .find(|selection| selection.id == selection_id)
        .ok_or(StoreError::NotFound)?;
    if !selection.active {
        return Err(StoreError::DisallowedNamespace);
    }
    transaction.commit().map_err(database::sql_error)?;
    Ok((device, selection, generation))
}

pub(crate) fn random_upload_id() -> StoreResult<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| StoreError::Random)?;
    let mut id = String::with_capacity(32);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        id.push(char::from(HEX[usize::from(byte >> 4)]));
        id.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(id)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
