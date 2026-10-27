mod builder;
mod committed;
mod lookup;

use drip_core::{Blake3Digest, SourcePath};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{DeviceIdentity, Store, StoreError, StoreResult, database, entry_files::EntrySnapshot};

pub(crate) use committed::record_committed_manifest;
pub(crate) use lookup::matching_offsets;

pub(crate) fn ensure_index(
    store: &Store,
    device: &DeviceIdentity,
    source: &SourcePath,
    revision: Option<u64>,
    baseline: Option<&EntrySnapshot>,
) -> StoreResult<bool> {
    let (Some(revision), Some(EntrySnapshot::File(snapshot))) = (revision, baseline) else {
        return Ok(false);
    };
    let revision_sql = super::state::sql_i64(revision)?;
    let size_sql = super::state::sql_i64(snapshot.size)?;
    if cached(
        store,
        &device.pc_id,
        source,
        revision_sql,
        size_sql,
        snapshot.digest,
    )? {
        return Ok(true);
    }
    let builder = crate::upload_store::random_upload_id()?;
    if !claim(
        store,
        &device.pc_id,
        source,
        revision_sql,
        size_sql,
        snapshot.digest,
        &builder,
    )? {
        return Ok(true);
    }
    let result = builder::build(store, device, source, revision_sql, snapshot, &builder);
    if result.is_err() {
        builder::discard(store, &device.pc_id, source, revision_sql, &builder);
    }
    result.map(|()| true)
}

fn cached(
    store: &Store,
    pc_id: &str,
    source: &SourcePath,
    revision: i64,
    size: i64,
    digest: Blake3Digest,
) -> StoreResult<bool> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let connection = store.connect()?;
    let row = connection
        .query_row(
            "SELECT file_size, digest, config_version, complete, builder_id \
             FROM chunk_manifests WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3",
            params![pc_id, source_json, revision],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(database::sql_error)?;
    match row {
        Some((stored_size, stored_digest, version, true, _))
            if stored_size == size
                && stored_digest == digest.to_hex()
                && version == i64::from(drip_core::CHUNK_CONFIG_VERSION) =>
        {
            valid_shape(&connection, pc_id, &source_json, revision, size)
        }
        Some((_, _, _, false, Some(_))) => Err(StoreError::Busy),
        _ => Ok(false),
    }
}

fn valid_shape(
    connection: &rusqlite::Connection,
    pc_id: &str,
    source_json: &str,
    revision: i64,
    size: i64,
) -> StoreResult<bool> {
    let mut statement = connection
        .prepare(
            "SELECT ordinal, offset, length, digest FROM file_chunks \
             WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3 ORDER BY ordinal",
        )
        .map_err(database::sql_error)?;
    let mut rows = statement
        .query(params![pc_id, source_json, revision])
        .map_err(database::sql_error)?;
    let mut ordinal = 0i64;
    let mut offset = 0i64;
    while let Some(row) = rows.next().map_err(database::sql_error)? {
        let current_ordinal = row.get::<_, i64>(0).map_err(database::sql_error)?;
        let current_offset = row.get::<_, i64>(1).map_err(database::sql_error)?;
        let length = row.get::<_, i64>(2).map_err(database::sql_error)?;
        let digest = row.get::<_, String>(3).map_err(database::sql_error)?;
        let Some(end) = current_offset.checked_add(length) else {
            return Ok(false);
        };
        if current_ordinal != ordinal
            || current_offset != offset
            || length <= 0
            || length > i64::from(drip_core::CHUNK_MAX_SIZE)
            || (end < size && length < i64::from(drip_core::CHUNK_MIN_SIZE))
            || end > size
            || Blake3Digest::from_hex(&digest).is_err()
        {
            return Ok(false);
        }
        ordinal += 1;
        offset = end;
    }
    Ok(if size == 0 {
        ordinal == 0
    } else {
        ordinal > 0 && offset == size
    })
}

fn claim(
    store: &Store,
    pc_id: &str,
    source: &SourcePath,
    revision: i64,
    size: i64,
    digest: Blake3Digest,
    builder: &str,
) -> StoreResult<bool> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let mut connection = store.connect()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database::sql_error)?;
    let current = transaction
        .query_row(
            "SELECT builder_id FROM chunk_manifests \
             WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3",
            params![pc_id, source_json, revision],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(database::sql_error)?;
    if current.flatten().is_some() {
        return Err(StoreError::Busy);
    }
    transaction
        .execute(
            "DELETE FROM chunk_manifests WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3",
            params![pc_id, source_json, revision],
        )
        .map_err(database::sql_error)?;
    transaction
        .execute(
            "INSERT INTO chunk_manifests(\
                 pc_id, source_json, revision, file_size, digest, config_version, complete, builder_id\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
            params![
                pc_id,
                source_json,
                revision,
                size,
                digest.to_hex(),
                i64::from(drip_core::CHUNK_CONFIG_VERSION),
                builder,
            ],
        )
        .map_err(database::sql_error)?;
    transaction.commit().map_err(database::sql_error)?;
    Ok(true)
}
