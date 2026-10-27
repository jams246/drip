use drip_core::SourcePath;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::entry_files::{DirectorySnapshot, EntrySnapshot};
use crate::{StoreError, StoreResult, database, metadata_revisions, upload_files::FileSnapshot};

struct StoredEntry {
    revision: i64,
    size: String,
    digest: String,
    seconds: i64,
    nanos: i64,
    change_seconds: i64,
    change_nanos: i64,
    device: String,
    inode: String,
}

pub(crate) fn record_entry_snapshot(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    snapshot: Option<&EntrySnapshot>,
) -> StoreResult<Option<u64>> {
    match snapshot {
        None => Ok(None),
        Some(EntrySnapshot::File(snapshot)) => {
            record_snapshot(transaction, pc_id, source, Some(snapshot))
        }
        Some(EntrySnapshot::Directory(snapshot)) => {
            record_directory_snapshot(transaction, pc_id, source, *snapshot).map(Some)
        }
    }
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
            "SELECT revision, file_size, digest, last_write_seconds, last_write_nanos, \
             change_seconds, change_nanos, device, inode \
             FROM entry_metadata WHERE pc_id = ?1 AND source_json = ?2",
            (&pc_id, &source_json),
            |row| {
                Ok(StoredEntry {
                    revision: row.get(0)?,
                    size: row.get(1)?,
                    digest: row.get(2)?,
                    seconds: row.get(3)?,
                    nanos: row.get(4)?,
                    change_seconds: row.get(5)?,
                    change_nanos: row.get(6)?,
                    device: row.get(7)?,
                    inode: row.get(8)?,
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
            && stored.change_seconds == snapshot.change_seconds
            && stored.change_nanos == snapshot.change_nanos
            && stored.device == snapshot.device.to_string()
            && stored.inode == snapshot.inode.to_string()
    });
    let revision = match stored.as_ref() {
        Some(stored) if matches => {
            metadata_revisions::retain(transaction, pc_id, source, stored.revision)?
        }
        Some(stored) => metadata_revisions::next(transaction, pc_id, source, stored.revision)?,
        None => metadata_revisions::next(transaction, pc_id, source, 0)?,
    };
    let revision_sql = i64::try_from(revision).map_err(|_| StoreError::Conflict)?;
    transaction
        .execute(
            "INSERT INTO entry_metadata(\
                 pc_id, source_json, revision, file_size, digest, last_write_seconds, \
                 last_write_nanos, change_seconds, change_nanos, device, inode\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
             ON CONFLICT(pc_id, source_json) DO UPDATE SET \
                 revision = excluded.revision, file_size = excluded.file_size, \
                 digest = excluded.digest, last_write_seconds = excluded.last_write_seconds, \
                 last_write_nanos = excluded.last_write_nanos, \
                 change_seconds = excluded.change_seconds, change_nanos = excluded.change_nanos, \
                 device = excluded.device, \
                 inode = excluded.inode",
            params![
                pc_id,
                source_json,
                revision_sql,
                snapshot.size.to_string(),
                snapshot.digest.to_hex(),
                snapshot.last_write.unix_seconds(),
                i64::from(snapshot.last_write.nanos()),
                snapshot.change_seconds,
                snapshot.change_nanos,
                snapshot.device.to_string(),
                snapshot.inode.to_string(),
            ],
        )
        .map_err(database::sql_error)?;
    transaction
        .execute(
            "DELETE FROM directory_metadata WHERE pc_id = ?1 AND source_json = ?2",
            (&pc_id, &source_json),
        )
        .map_err(database::sql_error)?;
    transaction
        .execute(
            "DELETE FROM chunk_manifests \
             WHERE pc_id = ?1 AND source_json = ?2 AND revision != ?3",
            params![pc_id, source_json, revision_sql],
        )
        .map_err(database::sql_error)?;
    Ok(Some(revision))
}

pub(crate) fn record_directory_snapshot(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
    snapshot: DirectorySnapshot,
) -> StoreResult<u64> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    let stored = transaction
        .query_row(
            "SELECT revision, device, inode, modified_seconds, modified_nanos, \
             change_seconds, change_nanos FROM directory_metadata \
             WHERE pc_id = ?1 AND source_json = ?2",
            (&pc_id, &source_json),
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()
        .map_err(database::sql_error)?;
    let minimum = stored.as_ref().map_or(0, |stored| stored.0);
    let matches = stored.as_ref().is_some_and(
        |(_, device, inode, seconds, nanos, change_seconds, change_nanos)| {
            device == &snapshot.device.to_string()
                && inode == &snapshot.inode.to_string()
                && *seconds == snapshot.modified.unix_seconds()
                && *nanos == i64::from(snapshot.modified.nanos())
                && *change_seconds == snapshot.change_seconds
                && *change_nanos == snapshot.change_nanos
        },
    );
    let revision = if matches {
        metadata_revisions::retain(transaction, pc_id, source, minimum)?
    } else {
        metadata_revisions::next(transaction, pc_id, source, minimum)?
    };
    let revision_sql = i64::try_from(revision).map_err(|_| StoreError::Conflict)?;
    transaction
        .execute(
            "INSERT INTO directory_metadata(\
                 pc_id, source_json, revision, device, inode, modified_seconds, \
                 modified_nanos, change_seconds, change_nanos\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(pc_id, source_json) DO UPDATE SET \
                 revision = excluded.revision, device = excluded.device, inode = excluded.inode, \
                 modified_seconds = excluded.modified_seconds, \
                 modified_nanos = excluded.modified_nanos, \
                 change_seconds = excluded.change_seconds, change_nanos = excluded.change_nanos",
            params![
                pc_id,
                source_json,
                revision_sql,
                snapshot.device.to_string(),
                snapshot.inode.to_string(),
                snapshot.modified.unix_seconds(),
                i64::from(snapshot.modified.nanos()),
                snapshot.change_seconds,
                snapshot.change_nanos,
            ],
        )
        .map_err(database::sql_error)?;
    transaction
        .execute(
            "DELETE FROM entry_metadata WHERE pc_id = ?1 AND source_json = ?2",
            (&pc_id, &source_json),
        )
        .map_err(database::sql_error)?;
    Ok(revision)
}

pub(crate) fn forget_entry(
    transaction: &Transaction<'_>,
    pc_id: &str,
    source: &SourcePath,
) -> StoreResult<()> {
    let source_json = serde_json::to_string(source).map_err(StoreError::Serialization)?;
    transaction
        .execute(
            "DELETE FROM chunk_manifests WHERE pc_id = ?1 AND source_json = ?2",
            (&pc_id, &source_json),
        )
        .map_err(database::sql_error)?;
    for table in ["entry_metadata", "directory_metadata"] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE pc_id = ?1 AND source_json = ?2"),
                (&pc_id, &source_json),
            )
            .map_err(database::sql_error)?;
    }
    Ok(())
}
