use drip_core::{ChunkDescriptor, SourcePath};
use rusqlite::{OptionalExtension, params};

use crate::{Store, StoreError, StoreResult, database};

pub(crate) fn matching_offsets(
    store: &Store,
    pc_id: &str,
    source: &SourcePath,
    revision: u64,
    descriptors: &[ChunkDescriptor],
) -> StoreResult<Vec<Option<u64>>> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let connection = store.connect()?;
    let mut statement = connection
        .prepare(
            "SELECT c.offset FROM file_chunks c JOIN chunk_manifests m \
             USING(pc_id, source_json, revision) \
             WHERE c.pc_id = ?1 AND c.source_json = ?2 AND c.revision = ?3 \
               AND c.digest = ?4 AND c.length = ?5 AND m.complete = 1 \
               AND m.config_version = ?6 ORDER BY c.offset LIMIT 1",
        )
        .map_err(database::sql_error)?;
    descriptors
        .iter()
        .map(|descriptor| {
            let offset = statement
                .query_row(
                    params![
                        pc_id,
                        source_json,
                        super::super::state::sql_i64(revision)?,
                        descriptor.digest.to_hex(),
                        i64::from(descriptor.length),
                        i64::from(drip_core::CHUNK_CONFIG_VERSION),
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(database::sql_error)?;
            offset
                .map(|value| u64::try_from(value).map_err(|_| StoreError::Conflict))
                .transpose()
        })
        .collect()
}
