use drip_core::SourcePath;
use rusqlite::{OptionalExtension, Transaction};

use crate::{StoreError, StoreResult, database};

pub(crate) fn retain(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    minimum: i64,
) -> StoreResult<u64> {
    let revision = current(transaction, pc_id, source)?
        .unwrap_or_default()
        .max(minimum);
    save(transaction, pc_id, source, revision)?;
    u64::try_from(revision).map_err(|_| StoreError::Conflict)
}

pub(crate) fn next(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    minimum: i64,
) -> StoreResult<u64> {
    let current = current(transaction, pc_id, source)?
        .unwrap_or_default()
        .max(minimum);
    let revision = current.checked_add(1).ok_or(StoreError::Conflict)?;
    save(transaction, pc_id, source, revision)?;
    u64::try_from(revision).map_err(|_| StoreError::Conflict)
}

fn current(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
) -> StoreResult<Option<i64>> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    transaction
        .query_row(
            "SELECT revision FROM entry_revisions WHERE pc_id = ?1 AND source_json = ?2",
            (pc_id, source_json),
            |row| row.get(0),
        )
        .optional()
        .map_err(database::sql_error)
}

fn save(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    revision: i64,
) -> StoreResult<()> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    transaction
        .execute(
            "INSERT INTO entry_revisions(pc_id, source_json, revision) VALUES (?1, ?2, ?3) \
             ON CONFLICT(pc_id, source_json) DO UPDATE SET revision = excluded.revision",
            (pc_id, source_json, revision),
        )
        .map_err(database::sql_error)?;
    Ok(())
}
