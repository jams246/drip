use drip_core::protocol::{
    EntryKind, Selection, SelectionSet, SelectionUpdate, SelectionUpdateResponse,
};
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
        let now = self.now()?;
        let device = crate::upload_store::authenticated_device(self, credential)?;
        let response = self.with_authorized_mutation(credential, |transaction, device| {
            apply_selection_update(transaction, device.pc_id.as_str(), update)
        })?;
        self.uploads
            .abort_obsolete(&device.pc_id, response.generation, now);
        Ok(response)
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
                "INSERT INTO selections(pc_id, selection_id, source_json, root_kind, active) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    &pc_id,
                    &selection.id,
                    source_json,
                    entry_kind_name(selection.root_kind),
                    &selection.active,
                ),
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
        if selection.active
            && selection.root_kind == EntryKind::File
            && selection.source.components().is_empty()
        {
            return Err(StoreError::InvalidInput);
        }
        if selections[..index]
            .iter()
            .any(|previous| previous.id == selection.id || previous.source == selection.source)
        {
            return Err(StoreError::InvalidInput);
        }
    }
    if active_count > 1 && has_overlapping_active_roots(selections) {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}

fn has_overlapping_active_roots(selections: &[Selection]) -> bool {
    let active = selections.iter().filter(|selection| selection.active);
    let active = active.collect::<Vec<_>>();
    active.iter().enumerate().any(|(index, left)| {
        active[index + 1..].iter().any(|right| {
            left.source.has_windows_prefix(&right.source)
                || right.source.has_windows_prefix(&left.source)
        })
    })
}

pub(crate) fn selection_contains_path(
    selection: &Selection,
    source: &drip_core::SourcePath,
) -> bool {
    selection.source.drive() == source.drive()
        && source.components().len() >= selection.source.components().len()
        && (selection.root_kind == EntryKind::Directory
            || source.components().len() == selection.source.components().len())
        && selection
            .source
            .components()
            .iter()
            .zip(source.components())
            .all(|(left, right)| left == right)
}

fn entry_kind_name(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Directory => "directory",
    }
}

#[cfg(test)]
mod tests;
