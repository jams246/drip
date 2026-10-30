use std::io::{Seek, SeekFrom};

use axum::body::Body;
use drip_core::protocol::UploadState;

use crate::{
    Store, StoreError, StoreResult, entry_files, upload_data::blocking,
    upload_store::authenticated_device,
};

use super::{lock, state};

impl Store {
    /// Accepts one declared missing chunk into its final staging range.
    pub async fn put_upload_chunk(
        &self,
        credential: &str,
        upload_id: &str,
        ordinal: u64,
        mut body: Body,
    ) -> StoreResult<()> {
        let store = self.clone();
        let secret = credential.to_owned();
        let id = upload_id.to_owned();
        let (device, session, details, descriptor, stage) = blocking(move || {
            let device = authenticated_device(&store, &secret)?;
            let now = store.now()?;
            store.reap_uploads(now)?;
            let session = store.uploads.get(&id, &device.pc_id, now)?;
            let mut current = lock(&session);
            if current.state != UploadState::Active
                || !current.delta_mode
                || current.planning
                || current.receiving
                || current.committing
            {
                return Err(StoreError::Conflict);
            }
            let descriptor = state::chunk_row(&store, &id, ordinal)?;
            if !descriptor.originally_missing {
                return Err(StoreError::Conflict);
            }
            let stage = if descriptor.accepted {
                None
            } else {
                let mut file = store.uploads.open_delta_stage(&current.stage_name)?;
                file.seek(SeekFrom::Start(descriptor.offset))
                    .map_err(super::io_error)?;
                Some(file.into_std())
            };
            current.receiving = true;
            let details = (
                current.stage_name.clone(),
                current.source_path.clone(),
                current.selection_root.clone(),
                current.selection_id.clone(),
                current.generation,
                current.base_snapshot.clone(),
                current.base_revision,
            );
            drop(current);
            Ok((device, session, details, descriptor, stage))
        })
        .await?;
        let (stage_name, source, root, selection_id, generation, baseline, revision) = details;
        let outcome =
            super::chunk_stream::receive(self, credential, &session, descriptor, stage, &mut body)
                .await;
        let store = self.clone();
        let credential = credential.to_owned();
        let upload_id = upload_id.to_owned();
        blocking(move || {
            if let Err(error) = outcome {
                let expired = {
                    let mut current = lock(&session);
                    current.receiving = false;
                    current.state == UploadState::Aborted
                };
                if expired {
                    let stage_result = store.uploads.remove_stage(&stage_name);
                    let plan_result = state::delete_plan(&store, &upload_id);
                    stage_result?;
                    plan_result?;
                }
                return Err(error);
            }
            let now = match store.now() {
                Ok(now) => now,
                Err(error) => {
                    lock(&session).receiving = false;
                    return Err(error);
                }
            };
            if descriptor.accepted {
                let mut current = lock(&session);
                if current.state != UploadState::Active || !current.receiving {
                    return Err(StoreError::Conflict);
                }
                current.receiving = false;
                current.last_progress = now;
                return Ok(());
            }
            let result = store.with_authorized_selection_mutation(
                &credential,
                &selection_id,
                generation,
                |transaction, current_device, selection, directory| {
                    if current_device.pc_id != device.pc_id
                        || selection.source != root
                        || !crate::selections::selection_contains_path(selection, &source)
                    {
                        return Err(StoreError::Conflict);
                    }
                    let mut current = lock(&session);
                    if current.state != UploadState::Active
                        || !current.receiving
                        || !current.delta_mode
                        || current.source_path != source
                        || current.selection_root != root
                        || current.base_revision != revision
                        || current.base_snapshot != baseline
                    {
                        return Err(StoreError::Conflict);
                    }
                    entry_files::check_entry(directory, &source, baseline.as_ref())?;
                    let observed_revision = crate::upload_metadata::record_entry_snapshot(
                        transaction,
                        &device.pc_id,
                        &source,
                        baseline.as_ref(),
                    )?;
                    if observed_revision != revision {
                        return Err(StoreError::Conflict);
                    }
                    state::mark_accepted(transaction, &upload_id, ordinal)?;
                    current.receiving = false;
                    current.last_progress = now;
                    Ok(())
                },
            );
            if result.is_err() {
                lock(&session).receiving = false;
            }
            result
        })
        .await
    }
}
