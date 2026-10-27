use drip_core::protocol::{Selection, SelectionSet};
use rusqlite::{OptionalExtension, Transaction};

use crate::{StoreError, StoreResult, database};

pub(crate) fn load_selections(
    transaction: &Transaction<'_>,
    pc_id: &str,
) -> StoreResult<SelectionSet> {
    let generation = transaction
        .query_row(
            "SELECT generation FROM device_state WHERE pc_id = ?1",
            [pc_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(database::sql_error)?
        .ok_or(StoreError::NotFound)?;
    let mut statement = transaction
        .prepare(
            "SELECT selection_id, source_json, root_kind, active FROM selections \
         WHERE pc_id = ?1 ORDER BY selection_id",
        )
        .map_err(database::sql_error)?;
    let rows = statement
        .query_map([pc_id], |row| {
            let source_json = row.get::<_, String>(1)?;
            let source = serde_json::from_str(&source_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    source_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let root_kind = row.get::<_, String>(2)?;
            let root_kind = serde_json::from_str(&format!("\"{root_kind}\"")).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    root_kind.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(Selection {
                id: row.get(0)?,
                source,
                root_kind,
                active: row.get(3)?,
            })
        })
        .map_err(database::sql_error)?;
    let selections = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(database::sql_error)?;
    Ok(SelectionSet {
        generation: u64::try_from(generation).map_err(|_| StoreError::Conflict)?,
        selections,
    })
}
