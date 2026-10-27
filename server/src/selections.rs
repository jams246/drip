mod read;
pub(crate) use read::load_selections;

use drip_core::protocol::{
    EntryKind, Selection, SelectionRootMove, SelectionSet, SelectionUpdate, SelectionUpdateResponse,
};
use rusqlite::{Transaction, TransactionBehavior};

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
        self.update_selections_relocating(credential, update, &[])
    }

    pub(crate) fn update_selections_relocating(
        &self,
        credential: &str,
        update: SelectionUpdate,
        root_moves: &[SelectionRootMove],
    ) -> StoreResult<SelectionUpdateResponse> {
        validate_selections(&update.selections)?;
        let now = self.now()?;
        let device = crate::upload_store::authenticated_device(self, credential)?;
        let response =
            self.with_authorized_device_mutation(credential, |transaction, device, directory| {
                apply_selection_update(
                    transaction,
                    device.pc_id.as_str(),
                    update,
                    directory,
                    root_moves,
                )
            })?;
        self.uploads
            .abort_obsolete(&device.pc_id, response.generation, now);
        Ok(response)
    }
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
    directory: &cap_std::fs::Dir,
    root_moves: &[SelectionRootMove],
) -> StoreResult<SelectionUpdateResponse> {
    let current = load_selections(transaction, pc_id)?;
    if current.generation != update.expected_generation {
        return Err(StoreError::Conflict);
    }
    for previous in &current.selections {
        if let Some(incoming) = update.selections.iter().find(|item| item.id == previous.id)
            && incoming.source != previous.source
            && !root_moves.iter().any(|moved| {
                moved.selection_id == previous.id && moved.previous_source == previous.source
            })
        {
            return Err(StoreError::Conflict);
        }
    }
    let mut selections = update.selections;
    for previous in &current.selections {
        if !selections.iter().any(|item| item.id == previous.id) {
            selections.push(Selection {
                active: false,
                ..previous.clone()
            });
        }
    }
    validate_selections(&selections)?;
    crate::selection_paths::relocate(directory, &current.selections, &selections, root_moves)?;
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
