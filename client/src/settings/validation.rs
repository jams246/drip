use std::collections::HashSet;

use super::{ClientSettings, selection::overlaps, store::SettingsError as StoreError};
use crate::source::same_path;

pub(super) fn validate(settings: &ClientSettings) -> Result<(), StoreError> {
    if settings.schema_version != 1 {
        return Err(StoreError::UnsupportedVersion);
    }
    if settings.pc_display_name.as_ref().is_some_and(|name| {
        name.is_empty() || name.len() > 128 || name.chars().any(char::is_control)
    }) {
        return Err(StoreError::InvalidState(
            "PC display name must contain 1 to 128 bytes without control characters",
        ));
    }
    if settings.start_at_sign_in != settings.startup_executable.is_some() {
        return Err(StoreError::InvalidState(
            "startup preference and registered executable must agree",
        ));
    }
    let mut ids = HashSet::with_capacity(settings.selections.len());
    for selection in &settings.selections {
        if selection.id.is_empty()
            || selection.volume_identity.is_empty()
            || !ids.insert(selection.id.as_str())
        {
            return Err(StoreError::InvalidState(
                "selection IDs must be unique and IDs and volume identities nonempty",
            ));
        }
    }
    for (index, selection) in settings.selections.iter().enumerate() {
        if settings
            .selections
            .iter()
            .skip(index + 1)
            .any(|other| same_path(&selection.source, &other.source))
        {
            return Err(StoreError::InvalidState(
                "retained selection paths must be unique",
            ));
        }
    }
    for (index, selection) in settings
        .selections
        .iter()
        .enumerate()
        .filter(|(_, item)| item.active)
    {
        if settings
            .selections
            .iter()
            .skip(index + 1)
            .any(|other| other.active && overlaps(&selection.source, &other.source))
        {
            return Err(StoreError::InvalidState("active selections overlap"));
        }
    }
    if settings
        .pending_update
        .as_ref()
        .is_some_and(|pending| pending.expected_generation != settings.acknowledged_generation)
    {
        return Err(StoreError::InvalidState(
            "pending selection generation does not match the acknowledged generation",
        ));
    }
    Ok(())
}
