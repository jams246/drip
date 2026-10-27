use crate::{
    Store, StoreError, StoreResult, selections, upload_files, upload_metadata,
    upload_store::authorized_active_selection,
};
use drip_core::protocol::{Entry, EntryKind, EntryPage};

impl Store {
    /// Lists actual files for the phase 01 single-file selection.
    pub fn list_entries(
        &self,
        credential: &str,
        selection_id: &str,
        cursor: Option<&str>,
    ) -> StoreResult<EntryPage> {
        let (device, selection) = authorized_active_selection(self, credential, selection_id)?;
        if cursor.is_some_and(|value| value != "end") {
            return Err(StoreError::InvalidInput);
        }
        if selection.source.components().is_empty() {
            return Err(StoreError::InvalidInput);
        }
        let snapshot =
            upload_files::read_snapshot(&self.open_device_directory(&device)?, &selection.source)?;
        let source = selection.source.clone();
        let entries =
            self.with_authorized_mutation(credential, |transaction, current_device| {
                let current = selections::load_selections(transaction, &current_device.pc_id)?;
                let active = current
                    .selections
                    .iter()
                    .find(|item| item.id == selection_id)
                    .ok_or(StoreError::NotFound)?;
                if !active.active {
                    return Err(StoreError::DisallowedNamespace);
                }
                if active.source != source {
                    return Err(StoreError::Conflict);
                }
                upload_files::check_snapshot(
                    &self.open_device_directory(current_device)?,
                    &source,
                    snapshot.as_ref(),
                )?;
                let revision = upload_metadata::record_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &source,
                    snapshot.as_ref(),
                )?;
                match (cursor.is_some(), snapshot.as_ref(), revision) {
                    (false, Some(snapshot), Some(revision)) => Ok(vec![Entry {
                        source_path: source.clone(),
                        kind: EntryKind::File,
                        size: snapshot.size,
                        digest: Some(snapshot.digest),
                        revision,
                        last_write: Some(snapshot.last_write),
                    }]),
                    _ => Ok(Vec::new()),
                }
            })?;
        Ok(EntryPage {
            selection_id: selection.id,
            entries,
            next_cursor: None,
        })
    }
}
