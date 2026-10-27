use drip_core::{Blake3Digest, protocol::UploadPlanResponse};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{Store, StoreError, StoreResult, database};

#[derive(Clone, Copy)]
pub(crate) struct ChunkRow {
    pub(crate) offset: u64,
    pub(crate) length: u32,
    pub(crate) digest: Blake3Digest,
    pub(crate) originally_missing: bool,
    pub(crate) accepted: bool,
}

pub(crate) fn delete_plan(store: &Store, upload_id: &str) -> StoreResult<()> {
    store.delete_upload_plans(&[upload_id.to_owned()])
}

pub(crate) fn find_page(
    store: &Store,
    upload_id: &str,
    request: &drip_core::protocol::UploadPlanRequest,
) -> StoreResult<Option<UploadPlanResponse>> {
    let start = request
        .descriptors
        .first()
        .map_or(0, |descriptor| descriptor.ordinal);
    let start = sql_i64(start)?;
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

pub(crate) fn chunk_row(store: &Store, upload_id: &str, ordinal: u64) -> StoreResult<ChunkRow> {
    let ordinal_sql = sql_i64(ordinal)?;
    let row = store
        .connect()?
        .query_row(
            "SELECT offset, length, digest, originally_missing, accepted FROM upload_chunks \
             WHERE upload_id = ?1 AND ordinal = ?2",
            params![upload_id, ordinal_sql],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()
        .map_err(database::sql_error)?
        .ok_or(StoreError::InvalidInput)?;
    let digest = Blake3Digest::from_hex(&row.2).map_err(|_| StoreError::Conflict)?;
    Ok(ChunkRow {
        offset: u64::try_from(row.0).map_err(|_| StoreError::Conflict)?,
        length: u32::try_from(row.1).map_err(|_| StoreError::Conflict)?,
        digest,
        originally_missing: row.3,
        accepted: row.4,
    })
}

pub(crate) fn mark_accepted(
    transaction: &Transaction<'_>,
    upload_id: &str,
    ordinal: u64,
) -> StoreResult<()> {
    let updated = transaction
        .execute(
            "UPDATE upload_chunks SET accepted = 1 WHERE upload_id = ?1 AND ordinal = ?2 \
             AND originally_missing = 1 AND accepted = 0",
            params![upload_id, sql_i64(ordinal)?],
        )
        .map_err(database::sql_error)?;
    if updated != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub(crate) fn require_complete(store: &Store, upload_id: &str, size: u64) -> StoreResult<()> {
    let connection = store.connect()?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(database::sql_error)?;
    require_complete_transaction(&transaction, upload_id, size)?;
    transaction.commit().map_err(database::sql_error)
}

pub(crate) fn require_complete_transaction(
    transaction: &Transaction<'_>,
    upload_id: &str,
    size: u64,
) -> StoreResult<()> {
    let size = sql_i64(size)?;
    let (pages, descriptors, min_version, max_version, page_descriptors) = transaction
        .query_row(
            "SELECT COUNT(*), SUM(descriptor_count), MIN(config_version), \
                    MAX(config_version), MAX(end_offset) \
             FROM upload_plan_pages WHERE upload_id = ?1",
            [upload_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                ))
            },
        )
        .map_err(database::sql_error)?;
    let coverage = transaction
        .query_row(
            "SELECT COUNT(*), MIN(ordinal), MAX(ordinal), MIN(offset), \
                    MAX(offset + length), SUM(length), \
                    SUM(CASE WHEN accepted = 0 THEN 1 ELSE 0 END) \
             FROM upload_chunks WHERE upload_id = ?1",
            [upload_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            },
        )
        .map_err(database::sql_error)?;
    let covered = if size == 0 {
        pages == 1
            && descriptors == 0
            && page_descriptors == 0
            && min_version == Some(i64::from(drip_core::CHUNK_CONFIG_VERSION))
            && max_version == min_version
            && coverage.0 == 0
    } else {
        pages > 0
            && descriptors == coverage.0
            && page_descriptors == size
            && min_version == Some(i64::from(drip_core::CHUNK_CONFIG_VERSION))
            && max_version == min_version
            && coverage.0 > 0
            && coverage.1 == Some(0)
            && coverage.2.and_then(|last| last.checked_add(1)) == Some(coverage.0)
            && coverage.3 == Some(0)
            && coverage.4 == Some(size)
            && coverage.5 == Some(size)
            && coverage.6 == Some(0)
    };
    if covered {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}

pub(crate) fn sql_i64(value: u64) -> StoreResult<i64> {
    i64::try_from(value).map_err(|_| StoreError::InvalidInput)
}
