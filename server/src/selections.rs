use drip_core::protocol::{Selection, SelectionSet, SelectionUpdate, SelectionUpdateResponse};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use crate::{DeviceIdentity, Store, StoreError, StoreResult, auth, database};

impl Store {
    /// Returns the authenticated device's active and retained selections.
    pub fn get_selections(&self, credential: &str) -> StoreResult<SelectionSet> {
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(database::sql_error)?;
        let device = auth::authenticate_transaction(&transaction, credential)?;
        let selections = load_selections(&transaction, &device.pc_id)?;
        transaction.commit().map_err(database::sql_error)?;
        Ok(selections)
    }

    /// Publishes a complete selection set after the current-generation barrier.
    pub fn update_selections(
        &self,
        credential: &str,
        update: SelectionUpdate,
    ) -> StoreResult<SelectionUpdateResponse> {
        validate_selections(&update.selections)?;
        self.with_authorized_mutation(credential, |transaction, device| {
            apply_selection_update(transaction, device.pc_id.as_str(), update)
        })
    }
}

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
            "SELECT selection_id, source_json, active FROM selections \
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
            Ok(Selection {
                id: row.get(0)?,
                source,
                active: row.get(2)?,
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

pub(crate) fn require_active_selection(
    transaction: &Transaction<'_>,
    device: &DeviceIdentity,
    selection_id: &str,
    expected_generation: u64,
) -> StoreResult<Selection> {
    if !auth::is_lower_hex(selection_id, 32) {
        return Err(StoreError::NotFound);
    }
    let current = load_selections(transaction, &device.pc_id)?;
    if current.generation != expected_generation {
        return Err(StoreError::Conflict);
    }
    let Some(selection) = current
        .selections
        .into_iter()
        .find(|selection| selection.id == selection_id)
    else {
        return Err(StoreError::NotFound);
    };
    if !selection.active {
        return Err(StoreError::DisallowedNamespace);
    }
    Ok(selection)
}

fn apply_selection_update(
    transaction: &Transaction<'_>,
    pc_id: &str,
    update: SelectionUpdate,
) -> StoreResult<SelectionUpdateResponse> {
    let current = load_selections(transaction, pc_id)?;
    if current.generation != update.expected_generation {
        return Err(StoreError::Conflict);
    }
    for previous in &current.selections {
        if let Some(incoming) = update.selections.iter().find(|item| item.id == previous.id)
            && incoming.source != previous.source
        {
            return Err(StoreError::Conflict);
        }
    }
    let mut selections = update.selections;
    for previous in current.selections {
        if !selections.iter().any(|item| item.id == previous.id) {
            selections.push(Selection {
                active: false,
                ..previous
            });
        }
    }
    validate_selections(&selections)?;
    let next = current
        .generation
        .checked_add(1)
        .ok_or(StoreError::Conflict)?;
    let next_sql = i64::try_from(next).map_err(|_| StoreError::Conflict)?;
    transaction
        .execute("DELETE FROM selections WHERE pc_id = ?1", [pc_id])
        .map_err(database::sql_error)?;
    for selection in selections {
        let source_json =
            serde_json::to_string(&selection.source).map_err(StoreError::Serialization)?;
        transaction
            .execute(
                "INSERT INTO selections(pc_id, selection_id, source_json, active) \
             VALUES (?1, ?2, ?3, ?4)",
                (&pc_id, &selection.id, source_json, &selection.active),
            )
            .map_err(database::sql_error)?;
    }
    transaction
        .execute(
            "UPDATE device_state SET generation = ?1 WHERE pc_id = ?2",
            (&next_sql, pc_id),
        )
        .map_err(database::sql_error)?;
    Ok(SelectionUpdateResponse { generation: next })
}

fn validate_selections(selections: &[Selection]) -> StoreResult<()> {
    let mut active_count = 0;
    for (index, selection) in selections.iter().enumerate() {
        if !auth::is_lower_hex(&selection.id, 32) {
            return Err(StoreError::InvalidInput);
        }
        active_count += usize::from(selection.active);
        if selections[..index]
            .iter()
            .any(|previous| previous.id == selection.id || previous.source == selection.source)
        {
            return Err(StoreError::InvalidInput);
        }
    }
    if active_count > 1 {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_selections;
    use drip_core::SourcePath;
    use drip_core::protocol::Selection;
    fn selection(id: &str, active: bool) -> Selection {
        Selection {
            id: id.to_owned(),
            source: SourcePath::new('C', vec!["file.txt".to_owned()]).unwrap(),
            active,
        }
    }

    #[test]
    fn selection_ids_must_be_lower_hex() {
        assert!(validate_selections(&[selection("A".repeat(32).as_str(), true)]).is_err());
    }

    #[test]
    fn selection_set_allows_at_most_one_active_file() {
        let first = selection(&"a".repeat(32), true);
        let mut second = selection(&"b".repeat(32), true);
        second.source = SourcePath::new('D', vec!["other.txt".to_owned()]).unwrap();
        assert!(validate_selections(&[first, second]).is_err());
    }
}
