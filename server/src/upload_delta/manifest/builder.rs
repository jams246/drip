use drip_core::{ChunkDescriptor, ChunkStream};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{
    DeviceIdentity, Store, StoreError, StoreResult, database,
    upload_files::{self, FileSnapshot},
    upload_paths,
};

const INDEX_PAGE: usize = 512;

pub(super) fn build(
    store: &Store,
    device: &DeviceIdentity,
    source: &drip_core::SourcePath,
    revision: i64,
    expected: &FileSnapshot,
    builder: &str,
) -> StoreResult<()> {
    let directory = store.open_device_directory(device)?;
    upload_files::check_snapshot(&directory, source, Some(expected))?;
    let (parent, name) =
        upload_paths::open_parent(&directory, source, false)?.ok_or(StoreError::Conflict)?;
    let file = upload_paths::open_file(&parent, &name)?.ok_or(StoreError::Conflict)?;
    upload_files::check_open_file(&file, expected)?;
    let identity = file.try_clone().map_err(super::super::io_error)?;
    let mut chunks = ChunkStream::new(file);
    let mut page = Vec::with_capacity(INDEX_PAGE);
    let mut total_size = 0u64;
    while let Some(chunk) = chunks.next_chunk().map_err(chunk_error)? {
        total_size = total_size
            .checked_add(u64::from(chunk.descriptor.length))
            .ok_or(StoreError::Conflict)?;
        page.push(chunk.descriptor);
        if page.len() == INDEX_PAGE {
            insert_batch(store, device, source, revision, builder, &page)?;
            page.clear();
        }
    }
    if !page.is_empty() {
        insert_batch(store, device, source, revision, builder, &page)?;
    }
    let digest = chunks.finish().map_err(chunk_error)?;
    if total_size != expected.size || digest != expected.digest {
        return Err(StoreError::Conflict);
    }
    upload_files::check_open_file(&identity, expected)?;
    upload_files::check_snapshot(&directory, source, Some(expected))?;
    finish_build(store, device, source, revision, expected, builder)
}

fn insert_batch(
    store: &Store,
    device: &DeviceIdentity,
    source: &drip_core::SourcePath,
    revision: i64,
    builder: &str,
    descriptors: &[ChunkDescriptor],
) -> StoreResult<()> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let mut connection = store.connect()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database::sql_error)?;
    let mut statement = transaction
        .prepare(
            "INSERT INTO file_chunks(\
                 pc_id, source_json, revision, ordinal, offset, length, digest\
             ) SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 \
               WHERE EXISTS (SELECT 1 FROM chunk_manifests WHERE pc_id = ?1 \
                 AND source_json = ?2 AND revision = ?3 AND builder_id = ?8)",
        )
        .map_err(database::sql_error)?;
    for descriptor in descriptors {
        statement
            .execute(params![
                device.pc_id,
                source_json,
                revision,
                super::super::state::sql_i64(descriptor.ordinal)?,
                super::super::state::sql_i64(descriptor.offset)?,
                i64::from(descriptor.length),
                descriptor.digest.to_hex(),
                builder,
            ])
            .map_err(database::sql_error)?;
    }
    drop(statement);
    transaction.commit().map_err(database::sql_error)
}

fn finish_build(
    store: &Store,
    device: &DeviceIdentity,
    source: &drip_core::SourcePath,
    revision: i64,
    expected: &FileSnapshot,
    builder: &str,
) -> StoreResult<()> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let mut connection = store.connect()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database::sql_error)?;
    let current = transaction
        .query_row(
            "SELECT file_size, digest FROM entry_metadata \
             WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3",
            params![device.pc_id, source_json, revision],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(database::sql_error)?;
    if current != Some((expected.size.to_string(), expected.digest.to_hex())) {
        return Err(StoreError::Conflict);
    }
    let updated = transaction
        .execute(
            "UPDATE chunk_manifests SET complete = 1, builder_id = NULL \
             WHERE pc_id = ?1 AND source_json = ?2 AND revision = ?3 AND builder_id = ?4",
            params![device.pc_id, source_json, revision, builder],
        )
        .map_err(database::sql_error)?;
    if updated != 1 {
        return Err(StoreError::Conflict);
    }
    transaction.commit().map_err(database::sql_error)
}

pub(super) fn discard(
    store: &Store,
    pc_id: &str,
    source: &drip_core::SourcePath,
    revision: i64,
    builder: &str,
) {
    let Ok(source_json) = serde_json::to_string(source) else {
        return;
    };
    let Ok(mut connection) = store.connect() else {
        return;
    };
    let Ok(transaction) = connection.transaction() else {
        return;
    };
    let _ = transaction.execute(
        "DELETE FROM chunk_manifests WHERE pc_id = ?1 AND source_json = ?2 \
         AND revision = ?3 AND builder_id = ?4",
        params![pc_id, source_json, revision, builder],
    );
    let _ = transaction.commit();
}

fn chunk_error(error: drip_core::ChunkStreamError) -> StoreError {
    match error {
        drip_core::ChunkStreamError::FastCdc(_) => StoreError::RetryableIo,
        _ => StoreError::Conflict,
    }
}
