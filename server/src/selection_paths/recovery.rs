use drip_core::SourcePath;
use rusqlite::{OptionalExtension, Transaction};

use crate::{StoreError, StoreResult, database, entry_files::EntrySnapshot};

pub(super) fn matches(
    transaction: &Transaction<'_>,
    pc_id: &str,
    old: &SourcePath,
    actual: &EntrySnapshot,
) -> StoreResult<bool> {
    let source = serde_json::to_string(old).map_err(StoreError::Serialization)?;
    match actual {
        EntrySnapshot::File(file) => {
            let stored = transaction.query_row(
                "SELECT device, inode, digest, last_write_seconds, last_write_nanos FROM entry_metadata WHERE pc_id = ?1 AND source_json = ?2",
                (pc_id, &source), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?)),
            ).optional().map_err(database::sql_error)?;
            Ok(
                stored.is_some_and(|(device, inode, digest, seconds, nanos)| {
                    device == file.device.to_string()
                        && inode == file.inode.to_string()
                        && digest == file.digest.to_hex()
                        && seconds == file.last_write.unix_seconds()
                        && nanos == i64::from(file.last_write.nanos())
                }),
            )
        }
        EntrySnapshot::Directory(directory) => {
            let stored = transaction.query_row(
                "SELECT device, inode, modified_seconds, modified_nanos FROM directory_metadata WHERE pc_id = ?1 AND source_json = ?2",
                (pc_id, &source), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?)),
            ).optional().map_err(database::sql_error)?;
            Ok(stored.is_some_and(|(device, inode, seconds, nanos)| {
                device == directory.device.to_string()
                    && inode == directory.inode.to_string()
                    && seconds == directory.modified.unix_seconds()
                    && nanos == i64::from(directory.modified.nanos())
            }))
        }
    }
}
