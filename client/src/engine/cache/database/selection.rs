use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::settings::SavedSelection;

use super::{Binding, DatabaseError};

pub(super) fn is_current(
    connection: &Connection,
    binding: &Binding,
    selections: &[SavedSelection],
) -> Result<bool, DatabaseError> {
    let Some(stored_binding) = read_binding(connection)? else {
        return Ok(false);
    };
    if !stored_binding.matches(binding) {
        return Ok(false);
    }
    let stored = {
        let mut statement =
            connection.prepare("SELECT selection_id, definition_json FROM cached_selections")?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?
    };
    let latest: HashMap<_, _> = selections
        .iter()
        .filter(|selection| selection.active)
        .map(|selection| (selection.id.as_str(), selection))
        .collect();
    if stored.len() != latest.len() {
        return Ok(false);
    }
    Ok(stored.iter().all(|(id, definition)| {
        latest.get(id.as_str()).is_some_and(|selection| {
            serde_json::from_str::<SavedSelection>(definition)
                .is_ok_and(|saved| saved.active && saved == **selection)
        })
    }))
}

pub(super) fn reconcile(
    connection: &mut Connection,
    binding: &Binding,
    selections: &[SavedSelection],
) -> Result<(), DatabaseError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored_binding = read_binding(&transaction)?;
    if stored_binding
        .as_ref()
        .is_some_and(|stored| stored.fingerprint_format != Binding::FINGERPRINT_FORMAT)
    {
        return Err(DatabaseError::Unsupported(
            "stored fingerprint format is unsupported".into(),
        ));
    }
    if stored_binding
        .as_ref()
        .is_none_or(|stored| !stored.matches(binding))
    {
        transaction.execute("DELETE FROM cached_selections", [])?;
        transaction.execute("DELETE FROM state_binding", [])?;
        insert_binding(&transaction, binding)?;
    }
    reconcile_selections(&transaction, selections)?;
    transaction.commit()?;
    Ok(())
}

struct StoredBinding {
    server_url: String,
    pc_id: String,
    mirror_directory: String,
    fingerprint_format: String,
}

impl StoredBinding {
    fn matches(&self, binding: &Binding) -> bool {
        self.server_url == binding.server_url
            && self.pc_id == binding.pc_id
            && self.mirror_directory == binding.mirror_directory
            && self.fingerprint_format == Binding::FINGERPRINT_FORMAT
    }
}

fn read_binding(connection: &Connection) -> Result<Option<StoredBinding>, DatabaseError> {
    Ok(connection
        .query_row(
            "SELECT server_url, pc_id, mirror_directory, fingerprint_format
             FROM state_binding WHERE singleton = 1",
            [],
            |row| {
                Ok(StoredBinding {
                    server_url: row.get(0)?,
                    pc_id: row.get(1)?,
                    mirror_directory: row.get(2)?,
                    fingerprint_format: row.get(3)?,
                })
            },
        )
        .optional()?)
}

fn insert_binding(connection: &Connection, binding: &Binding) -> Result<(), DatabaseError> {
    connection.execute(
        "INSERT INTO state_binding
         (singleton, server_url, pc_id, mirror_directory, fingerprint_format)
         VALUES (1, ?1, ?2, ?3, ?4)",
        (
            &binding.server_url,
            &binding.pc_id,
            &binding.mirror_directory,
            Binding::FINGERPRINT_FORMAT,
        ),
    )?;
    Ok(())
}

fn reconcile_selections(
    connection: &Connection,
    selections: &[SavedSelection],
) -> Result<(), DatabaseError> {
    let latest: HashMap<_, _> = selections
        .iter()
        .filter(|selection| selection.active)
        .map(|selection| (selection.id.as_str(), selection))
        .collect();
    let stored = {
        let mut statement =
            connection.prepare("SELECT selection_id, definition_json FROM cached_selections")?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, definition) in stored {
        let unchanged = latest.get(id.as_str()).is_some_and(|selection| {
            serde_json::from_str::<SavedSelection>(&definition)
                .is_ok_and(|saved| saved.active && saved == **selection)
        });
        if !unchanged {
            connection.execute(
                "DELETE FROM cached_selections WHERE selection_id = ?1",
                [&id],
            )?;
        }
    }
    for selection in latest.values() {
        let definition = serde_json::to_string(selection)
            .map_err(|error| DatabaseError::Unsupported(error.to_string()))?;
        connection.execute(
            "INSERT OR IGNORE INTO cached_selections (selection_id, definition_json)
             VALUES (?1, ?2)",
            (&selection.id, &definition),
        )?;
    }
    Ok(())
}
