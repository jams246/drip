use drip_core::protocol::{UploadPlanRequest, UploadPlanResponse};

use rusqlite::{OptionalExtension, params};

use crate::{Store, StoreError, StoreResult, database, upload_store::authenticated_device};

use super::{accept, lock};

pub(crate) fn find_page(
    store: &Store,
    upload_id: &str,
    request: &UploadPlanRequest,
) -> StoreResult<Option<UploadPlanResponse>> {
    let start = request
        .descriptors
        .first()
        .map_or(0, |descriptor| descriptor.ordinal);
    let start = super::state::sql_i64(start)?;
    let connection = store.connect()?;
    let page = connection
        .query_row(
            "SELECT descriptor_count, config_version FROM upload_plan_pages \
             WHERE upload_id = ?1 AND first_ordinal = ?2",
            params![upload_id, start],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(database::sql_error)?;
    let Some((count, version)) = page else {
        return Ok(None);
    };
    if usize::try_from(count).ok() != Some(request.descriptors.len())
        || version != i64::from(request.config.chunking_version)
    {
        return Err(StoreError::Conflict);
    }
    let mut statement = connection
        .prepare(
            "SELECT ordinal, offset, length, digest, originally_missing FROM upload_chunks \
             WHERE upload_id = ?1 AND page_start = ?2 ORDER BY ordinal",
        )
        .map_err(database::sql_error)?;
    let mut rows = statement
        .query(params![upload_id, start])
        .map_err(database::sql_error)?;
    let mut missing_ordinals = Vec::new();
    for requested in &request.descriptors {
        let Some(stored) = rows.next().map_err(database::sql_error)? else {
            return Err(StoreError::Conflict);
        };
        let ordinal = stored.get::<_, i64>(0).map_err(database::sql_error)?;
        let offset = stored.get::<_, i64>(1).map_err(database::sql_error)?;
        let length = stored.get::<_, i64>(2).map_err(database::sql_error)?;
        let digest = stored.get::<_, String>(3).map_err(database::sql_error)?;
        let missing = stored.get::<_, bool>(4).map_err(database::sql_error)?;
        if u64::try_from(ordinal).ok() != Some(requested.ordinal)
            || u64::try_from(offset).ok() != Some(requested.offset)
            || u32::try_from(length).ok() != Some(requested.length)
            || digest != requested.digest.to_hex()
        {
            return Err(StoreError::Conflict);
        }
        if missing {
            missing_ordinals.push(requested.ordinal);
        }
    }
    if rows.next().map_err(database::sql_error)?.is_some() {
        return Err(StoreError::Conflict);
    }
    Ok(Some(UploadPlanResponse { missing_ordinals }))
}

pub(crate) fn cursor(store: &Store, upload_id: &str) -> StoreResult<(u64, u64, bool)> {
    let connection = store.connect()?;
    let pending = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM upload_chunks \
             WHERE upload_id = ?1 AND originally_missing = 1 AND accepted = 0)",
            [upload_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(database::sql_error)?;
    let last = connection
        .query_row(
            "SELECT ordinal, offset, length FROM upload_chunks \
             WHERE upload_id = ?1 ORDER BY ordinal DESC LIMIT 1",
            [upload_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(database::sql_error)?;
    match last {
        Some((ordinal, offset, length)) => {
            let next_ordinal = u64::try_from(ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(1))
                .ok_or(StoreError::Conflict)?;
            let next_offset = u64::try_from(offset)
                .ok()
                .and_then(|offset| offset.checked_add(u64::try_from(length).ok()?))
                .ok_or(StoreError::Conflict)?;
            Ok((next_ordinal, next_offset, pending))
        }
        None => Ok((0, 0, pending)),
    }
}

impl Store {
    /// Accepts one bounded descriptor page for an active upload session.
    pub fn plan_upload(
        &self,
        credential: &str,
        upload_id: &str,
        request: UploadPlanRequest,
    ) -> StoreResult<UploadPlanResponse> {
        let now = self.now()?;
        self.reap_uploads(now)?;
        let device = authenticated_device(self, credential)?;
        let session = self.uploads.get(upload_id, &device.pc_id, now)?;
        {
            let mut current = lock(&session);
            if current.state != drip_core::protocol::UploadState::Active
                || current.receiving
                || current.committing
                || current.planning
                || current.has_data
            {
                return Err(StoreError::Conflict);
            }
            current.planning = true;
        }
        let result = accept::plan_page(self, credential, upload_id, &session, request);
        let mut current = lock(&session);
        current.planning = false;
        if result.is_ok() && current.state == drip_core::protocol::UploadState::Active {
            current.delta_mode = true;
            current.last_progress = now;
        }
        if current.state != drip_core::protocol::UploadState::Active {
            return Err(StoreError::Conflict);
        }
        result
    }
}
