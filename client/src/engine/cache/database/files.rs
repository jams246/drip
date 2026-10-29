use std::collections::HashSet;

use drip_core::{Blake3Digest, SourcePath, Timestamp};
use rusqlite::{Connection, TransactionBehavior, types::Type};

use crate::settings::SavedSelection;

use super::{CacheRecord, DatabaseError, path_belongs_to_selection};

struct StoredRecord {
    source_json: String,
    volume_identity: String,
    volume_serial: Vec<u8>,
    file_id: Vec<u8>,
    file_size: i64,
    last_write_seconds: i64,
    last_write_nanos: i64,
    change_time: i64,
    digest: Vec<u8>,
    verified_seconds: i64,
    verified_nanos: i64,
}

pub(super) fn get(
    connection: &mut Connection,
    database_path: &std::path::Path,
    selection: &SavedSelection,
    source_json: &str,
) -> Result<Option<CacheRecord>, DatabaseError> {
    let stored = connection.query_row(
        "SELECT source_json, volume_identity, volume_serial, file_id, file_size,
                last_write_seconds, last_write_nanos, change_time, digest,
                verified_seconds, verified_nanos
         FROM cached_files WHERE selection_id = ?1 AND source_json = ?2",
        (&selection.id, source_json),
        |row| {
            Ok(StoredRecord {
                source_json: row.get(0)?,
                volume_identity: row.get(1)?,
                volume_serial: row.get(2)?,
                file_id: row.get(3)?,
                file_size: row.get(4)?,
                last_write_seconds: row.get(5)?,
                last_write_nanos: row.get(6)?,
                change_time: row.get(7)?,
                digest: row.get(8)?,
                verified_seconds: row.get(9)?,
                verified_nanos: row.get(10)?,
            })
        },
    );
    let stored = match stored {
        Ok(stored) => Some(stored),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) if invalid_row(&error) => {
            super::recovery::prepare_journal(database_path)?;
            delete(connection, &selection.id, source_json)?;
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let Some(stored) = stored else {
        return Ok(None);
    };
    let Some(record) = decode(stored, selection) else {
        super::recovery::prepare_journal(database_path)?;
        delete(connection, &selection.id, source_json)?;
        return Ok(None);
    };
    Ok(Some(record))
}

pub(super) fn delete(
    connection: &Connection,
    selection_id: &str,
    source_json: &str,
) -> Result<(), DatabaseError> {
    connection.execute(
        "DELETE FROM cached_files WHERE selection_id = ?1 AND source_json = ?2",
        (selection_id, source_json),
    )?;
    Ok(())
}

pub(super) fn save(
    connection: &mut Connection,
    selection: &SavedSelection,
    path: &SourcePath,
    record: &CacheRecord,
) -> Result<(), DatabaseError> {
    if !selection.active
        || !path_belongs_to_selection(path, selection)
        || record.path != *path
        || record.volume_identity != selection.volume_identity
        || record.observation.change_time == 0
    {
        return Ok(());
    }
    let source_json = serde_json::to_string(path)
        .map_err(|error| DatabaseError::Unsupported(error.to_string()))?;
    let file_size = i64::try_from(record.observation.size)
        .map_err(|error| DatabaseError::Unsupported(format!("file size out of range: {error}")))?;
    // Store unsigned volume serials as big-endian bytes without signed truncation.
    let serial = record.observation.volume.to_be_bytes();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT INTO cached_files
         (selection_id, source_json, volume_identity, volume_serial, file_id,
          file_size, last_write_seconds, last_write_nanos, change_time, digest,
          verified_seconds, verified_nanos)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(selection_id, source_json) DO UPDATE SET
          volume_identity = excluded.volume_identity,
          volume_serial = excluded.volume_serial,
          file_id = excluded.file_id,
          file_size = excluded.file_size,
          last_write_seconds = excluded.last_write_seconds,
          last_write_nanos = excluded.last_write_nanos,
          change_time = excluded.change_time,
          digest = excluded.digest,
          verified_seconds = excluded.verified_seconds,
          verified_nanos = excluded.verified_nanos",
        (
            &selection.id,
            &source_json,
            &record.volume_identity,
            serial.as_slice(),
            record.observation.file_id.as_slice(),
            file_size,
            record.observation.last_write.unix_seconds(),
            i64::from(record.observation.last_write.nanos()),
            record.observation.change_time,
            record.digest.as_bytes().as_slice(),
            record.verified_at.unix_seconds(),
            i64::from(record.verified_at.nanos()),
        ),
    )?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn prune(
    connection: &mut Connection,
    selection_id: &str,
    paths: &[String],
) -> Result<(), DatabaseError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing = {
        let mut statement =
            transaction.prepare("SELECT source_json FROM cached_files WHERE selection_id = ?1")?;
        statement
            .query_map([selection_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let keep: HashSet<_> = paths.iter().map(String::as_str).collect();
    for path in existing {
        if !keep.contains(path.as_str()) {
            transaction.execute(
                "DELETE FROM cached_files WHERE selection_id = ?1 AND source_json = ?2",
                (selection_id, path),
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

fn decode(stored: StoredRecord, selection: &SavedSelection) -> Option<CacheRecord> {
    let path: SourcePath = serde_json::from_str(&stored.source_json).ok()?;
    if !path_belongs_to_selection(&path, selection)
        || stored.volume_identity != selection.volume_identity
        || stored.volume_serial.len() != 8
        || stored.file_id.len() != 16
        || stored.digest.len() != 32
        || stored.change_time == 0
        || stored.verified_seconds < 0
    {
        return None;
    }
    let volume = u64::from_be_bytes(stored.volume_serial.as_slice().try_into().ok()?);
    let file_id = stored.file_id.as_slice().try_into().ok()?;
    let digest = Blake3Digest::new(stored.digest.as_slice().try_into().ok()?);
    let size = u64::try_from(stored.file_size).ok()?;
    let last_write_nanos = u32::try_from(stored.last_write_nanos).ok()?;
    let verified_nanos = u32::try_from(stored.verified_nanos).ok()?;
    let last_write = Timestamp::new(stored.last_write_seconds, last_write_nanos).ok()?;
    let verified_at = Timestamp::new(stored.verified_seconds, verified_nanos).ok()?;
    Some(CacheRecord {
        path,
        observation: crate::source::Observation {
            volume,
            file_id,
            size,
            last_write,
            change_time: stored.change_time,
        },
        volume_identity: stored.volume_identity,
        digest,
        verified_at,
    })
}

fn invalid_row(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::FromSqlConversionFailure(_, _, _)
            | rusqlite::Error::InvalidColumnType(_, _, Type::Null)
            | rusqlite::Error::InvalidColumnType(_, _, Type::Integer)
            | rusqlite::Error::InvalidColumnType(_, _, Type::Real)
            | rusqlite::Error::InvalidColumnType(_, _, Type::Text)
            | rusqlite::Error::InvalidColumnType(_, _, Type::Blob)
    )
}
