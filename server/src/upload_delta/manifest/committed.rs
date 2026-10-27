use drip_core::{Blake3Digest, SourcePath};
use rusqlite::{Transaction, params};

use crate::{StoreError, StoreResult, database};

pub(crate) fn record_committed_manifest(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    revision: u64,
    size: u64,
    digest: Blake3Digest,
    upload_id: &str,
) -> StoreResult<()> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let revision = super::super::state::sql_i64(revision)?;
    let size = super::super::state::sql_i64(size)?;
    transaction
        .execute(
            "INSERT INTO chunk_manifests(\
                 pc_id, source_json, revision, file_size, digest, config_version, complete, builder_id\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, NULL)",
            params![
                pc_id,
                source_json,
                revision,
                size,
                digest.to_hex(),
                i64::from(drip_core::CHUNK_CONFIG_VERSION),
            ],
        )
        .map_err(database::sql_error)?;
    transaction
        .execute(
            "INSERT INTO file_chunks(\
                 pc_id, source_json, revision, ordinal, offset, length, digest\
             ) SELECT ?1, ?2, ?3, ordinal, offset, length, digest \
               FROM upload_chunks WHERE upload_id = ?4 ORDER BY ordinal",
            params![pc_id, source_json, revision, upload_id],
        )
        .map_err(database::sql_error)?;
    transaction
        .execute(
            "DELETE FROM upload_plan_pages WHERE upload_id = ?1",
            [upload_id],
        )
        .map_err(database::sql_error)?;
    Ok(())
}
