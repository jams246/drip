use std::sync::{Arc, Mutex};

use drip_core::protocol::{UploadPlanRequest, UploadPlanResponse};
use rusqlite::params;

use crate::{
    Store, StoreError, StoreResult, database, entry_files, upload_sessions::UploadSession,
    upload_store::authorized_selection,
};

use super::{lock, manifest, pages, ranges, state};

pub(super) fn plan_page(
    store: &Store,
    credential: &str,
    upload_id: &str,
    session: &Arc<Mutex<UploadSession>>,
    request: UploadPlanRequest,
) -> StoreResult<UploadPlanResponse> {
    let (selection_id, generation, expected_size, source, root, revision, baseline, stage_name) = {
        let current = lock(session);
        (
            current.selection_id.clone(),
            current.generation,
            current.expected_size,
            current.source_path.clone(),
            current.selection_root.clone(),
            current.base_revision,
            current.base_snapshot.clone(),
            current.stage_name.clone(),
        )
    };
    let (device, selection) = authorized_selection(store, credential, &selection_id, generation)?;
    if selection.source != root || !crate::selections::selection_contains_path(&selection, &source)
    {
        return Err(StoreError::Conflict);
    }
    request
        .config
        .validate()
        .map_err(|_| StoreError::InvalidInput)?;
    if let Some(response) = pages::find_page(store, upload_id, &request)? {
        return Ok(response);
    }
    let (next_ordinal, next_offset, pending) = pages::cursor(store, upload_id)?;
    if pending {
        return Err(StoreError::Conflict);
    }
    request
        .validate(expected_size, next_ordinal, next_offset)
        .map_err(|_| StoreError::InvalidInput)?;
    state::sql_i64(expected_size)?;
    let manifest_ready =
        match manifest::ensure_index(store, &device, &source, revision, baseline.as_ref()) {
            Ok(ready) => ready,
            Err(StoreError::RetryableIo) => false,
            Err(error) => return Err(error),
        };
    let descriptors = &request.descriptors;
    let offsets = if manifest_ready {
        manifest::matching_offsets(
            store,
            &device.pc_id,
            &source,
            revision.ok_or(StoreError::Conflict)?,
            descriptors,
        )?
    } else {
        vec![None; descriptors.len()]
    };
    let directory = store.open_device_directory(&device)?;
    let expected_baseline = match baseline.as_ref() {
        Some(crate::entry_files::EntrySnapshot::File(snapshot)) => Some(snapshot),
        _ => None,
    };
    let mut baseline_file = if manifest_ready {
        ranges::baseline_file(&directory, &source, expected_baseline)
    } else {
        None
    };
    let mut stage = store.uploads.open_delta_stage(&stage_name)?;
    stage.set_len(expected_size).map_err(super::io_error)?;
    let mut planned = Vec::with_capacity(descriptors.len());
    for (descriptor, candidate) in descriptors.iter().copied().zip(offsets) {
        let copied = match (baseline_file.as_mut(), candidate) {
            (Some(file), Some(offset)) => ranges::copy_range(
                file,
                &mut stage,
                offset,
                descriptor.offset,
                descriptor.length,
                descriptor.digest,
            )?,
            _ => false,
        };
        planned.push((
            descriptor,
            if copied { candidate } else { None },
            !copied,
            copied,
        ));
    }
    stage.sync_all().map_err(super::io_error)?;
    let end_offset = match descriptors.last() {
        Some(last) => last
            .offset
            .checked_add(u64::from(last.length))
            .ok_or(StoreError::InvalidInput)?,
        None => next_offset,
    };
    let start_ordinal = descriptors.first().map_or(0, |item| item.ordinal);
    store.with_authorized_selection_mutation(
        credential,
        &selection_id,
        generation,
        |transaction, current_device, current_selection, current_directory| {
            if current_device.pc_id != device.pc_id
                || current_selection.source != root
                || !crate::selections::selection_contains_path(current_selection, &source)
            {
                return Err(StoreError::Conflict);
            }
            let current = lock(session);
            if current.state != drip_core::protocol::UploadState::Active
                || !current.planning
                || current.receiving
                || current.committing
                || current.has_data
                || current.source_path != source
                || current.selection_root != root
                || current.base_revision != revision
                || current.base_snapshot != baseline
            {
                return Err(StoreError::Conflict);
            }
            entry_files::check_entry(current_directory, &source, baseline.as_ref())?;
            let observed_revision = crate::upload_metadata::record_entry_snapshot(
                transaction,
                &device.pc_id,
                &source,
                baseline.as_ref(),
            )?;
            if observed_revision != revision {
                return Err(StoreError::Conflict);
            }
            transaction
                .execute(
                    "INSERT INTO upload_plan_pages(\
                         upload_id, first_ordinal, descriptor_count, end_offset, config_version\
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        upload_id,
                        state::sql_i64(start_ordinal)?,
                        i64::try_from(descriptors.len()).map_err(|_| StoreError::InvalidInput)?,
                        state::sql_i64(end_offset)?,
                        i64::from(request.config.chunking_version),
                    ],
                )
                .map_err(database::sql_error)?;
            for (descriptor, base_offset, missing, accepted) in &planned {
                transaction
                    .execute(
                        "INSERT INTO upload_chunks(\
                             upload_id, page_start, ordinal, offset, length, digest, \
                             baseline_offset, originally_missing, accepted\
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            upload_id,
                            state::sql_i64(start_ordinal)?,
                            state::sql_i64(descriptor.ordinal)?,
                            state::sql_i64(descriptor.offset)?,
                            i64::from(descriptor.length),
                            descriptor.digest.to_hex(),
                            base_offset.map(state::sql_i64).transpose()?,
                            missing,
                            accepted,
                        ],
                    )
                    .map_err(database::sql_error)?;
            }
            Ok(UploadPlanResponse {
                missing_ordinals: planned
                    .iter()
                    .filter_map(|(item, _, missing, _)| missing.then_some(item.ordinal))
                    .collect(),
            })
        },
    )
}
