use drip_core::SourcePath;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{StoreError, StoreResult, database, upload_files::FileSnapshot};

struct StoredEntry {
    revision: i64,
    size: String,
    digest: String,
    seconds: i64,
    nanos: i64,
    device: String,
    inode: String,
}

pub(crate) fn record_snapshot(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    snapshot: Option<&FileSnapshot>,
) -> StoreResult<Option<u64>> {
    let Some(snapshot) = snapshot else {
        return Ok(None);
    };
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let stored = transaction
        .query_row(
            "SELECT revision, file_size, digest, last_write_seconds, last_write_nanos, device, inode \
             FROM entry_metadata WHERE pc_id = ?1 AND source_json = ?2",
            (&pc_id, &source_json),
            |row| {
                Ok(StoredEntry {
                    revision: row.get(0)?,
                    size: row.get(1)?,
                    digest: row.get(2)?,
                    seconds: row.get(3)?,
                    nanos: row.get(4)?,
                    device: row.get(5)?,
                    inode: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(database::sql_error)?;
    let matches = stored.as_ref().is_some_and(|stored| {
        stored.size == snapshot.size.to_string()
            && stored.digest == snapshot.digest.to_hex()
            && stored.seconds == snapshot.last_write.unix_seconds()
            && stored.nanos == i64::from(snapshot.last_write.nanos())
            && stored.device == snapshot.device.to_string()
            && stored.inode == snapshot.inode.to_string()
    });
    let revision = match stored {
        Some(stored) if matches => {
            u64::try_from(stored.revision).map_err(|_| StoreError::Conflict)?
        }
        Some(stored) => u64::try_from(stored.revision.checked_add(1).ok_or(StoreError::Conflict)?)
            .map_err(|_| StoreError::Conflict)?,
        None => 1,
    };
    let revision_sql = i64::try_from(revision).map_err(|_| StoreError::Conflict)?;
    transaction
        .execute(
            "INSERT INTO entry_metadata(\
                 pc_id, source_json, revision, file_size, digest, last_write_seconds, \
                 last_write_nanos, device, inode\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(pc_id, source_json) DO UPDATE SET \
                 revision = excluded.revision, file_size = excluded.file_size, \
                 digest = excluded.digest, last_write_seconds = excluded.last_write_seconds, \
                 last_write_nanos = excluded.last_write_nanos, device = excluded.device, \
                 inode = excluded.inode",
            params![
                pc_id,
                source_json,
                revision_sql,
                snapshot.size.to_string(),
                snapshot.digest.to_hex(),
                snapshot.last_write.unix_seconds(),
                i64::from(snapshot.last_write.nanos()),
                snapshot.device.to_string(),
                snapshot.inode.to_string(),
            ],
        )
        .map_err(database::sql_error)?;
    Ok(Some(revision))
}
