use std::{path::Path, time::Duration};

use rusqlite::{Connection, Error, ErrorCode};

use crate::StoreError;

mod schema;
pub(crate) use schema::initialize;

const BUSY_TIMEOUT: Duration = Duration::from_millis(400);

pub(crate) fn connect(path: &Path) -> Result<Connection, StoreError> {
    let connection = Connection::open(path).map_err(sql_error)?;
    connection.busy_timeout(BUSY_TIMEOUT).map_err(sql_error)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(sql_error)?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(sql_error)?;
    Ok(connection)
}

pub(crate) fn sql_error(error: Error) -> StoreError {
    if matches!(
        &error,
        Error::SqliteFailure(code, _)
            if matches!(code.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    ) {
        StoreError::Busy
    } else {
        StoreError::Database(error)
    }
}

pub(crate) fn io_error(error: std::io::Error) -> StoreError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        StoreError::InsufficientStagingSpace
    } else {
        StoreError::Filesystem(error)
    }
}

#[cfg(test)]
mod tests;
