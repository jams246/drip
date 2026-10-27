use drip_core::{SourcePath, protocol::Selection};
use thiserror::Error;

use super::{ClientSettings, PendingSelectionUpdate, SavedSelection};
use crate::{
    settings::store::SettingsError as StoreError,
    source::{SourceError, SourceRoot, random_id, same_path},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddSelectionResult {
    pub selection: SavedSelection,
    pub authority_over: Vec<SourcePath>,
}

#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("source selection overlaps active selection {0}")]
    Overlap(String),
    #[error("selection not found: {0}")]
    NotFound(String),
    #[error("could not create a unique selection ID")]
    IdExhausted,
    #[error("selection generation overflowed")]
    GenerationOverflow,
    #[error("could not inspect source root: {0}")]
    Source(#[from] SourceError),
}

pub(super) fn add(
    settings: &mut ClientSettings,
    root: SourceRoot,
) -> Result<AddSelectionResult, StoreError> {
    let existing = settings
        .selections
        .iter()
        .position(|item| same_path(&item.source, &root.source));
    if existing.is_some_and(|index| settings.selections[index].active) {
        return Err(SelectionError::Overlap(root.source.to_windows_path()).into());
    }
    for item in &settings.selections {
        if item.active
            && !existing.is_some_and(|index| settings.selections[index].id == item.id)
            && overlaps(&item.source, &root.source)
        {
            return Err(SelectionError::Overlap(item.source.to_windows_path()).into());
        }
    }
    let authority_over = settings
        .selections
        .iter()
        .filter(|item| !item.active && strict_ancestor(&root.source, &item.source))
        .map(|item| item.source.clone())
        .collect();
    let selected = match existing {
        Some(index) => {
            let mut item = settings.selections[index].clone();
            item.source = root.source;
            item.root_kind = root.kind;
            item.volume_identity = root.volume_identity;
            item.active = true;
            item
        }
        None => SavedSelection {
            id: unique_id(settings)?,
            source: root.source,
            root_kind: root.kind,
            volume_identity: root.volume_identity,
            active: true,
        },
    };
    if let Some(index) = existing {
        settings.selections[index] = selected.clone();
    } else {
        settings.selections.push(selected.clone());
    }
    stage(settings)?;
    Ok(AddSelectionResult {
        selection: selected,
        authority_over,
    })
}

pub(super) fn remove(
    settings: &mut ClientSettings,
    source: &SourcePath,
) -> Result<bool, StoreError> {
    let Some(selection) = settings
        .selections
        .iter_mut()
        .find(|item| same_path(&item.source, source))
    else {
        return Err(SelectionError::NotFound(source.to_windows_path()).into());
    };
    if !selection.active {
        return Ok(false);
    }
    selection.active = false;
    stage(settings)?;
    Ok(true)
}

pub(super) fn refresh_root_kind(
    settings: &mut ClientSettings,
    id: &str,
    kind: drip_core::protocol::EntryKind,
) -> Result<bool, StoreError> {
    let Some(selection) = settings.selections.iter_mut().find(|item| item.id == id) else {
        return Err(SelectionError::NotFound(id.to_owned()).into());
    };
    if selection.root_kind == kind {
        return Ok(false);
    }
    selection.root_kind = kind;
    stage(settings)?;
    Ok(true)
}

fn stage(settings: &mut ClientSettings) -> Result<(), SelectionError> {
    let expected_generation = settings
        .pending_update
        .as_ref()
        .map_or(settings.acknowledged_generation, |pending| {
            pending.expected_generation
        });
    expected_generation
        .checked_add(1)
        .ok_or(SelectionError::GenerationOverflow)?;
    settings.pending_update = Some(PendingSelectionUpdate {
        expected_generation,
        selections: settings
            .selections
            .iter()
            .map(|item| Selection {
                id: item.id.clone(),
                source: item.source.clone(),
                active: item.active,
                root_kind: item.root_kind,
            })
            .collect(),
    });
    Ok(())
}

pub(super) fn restage_after_ack(
    settings: &mut ClientSettings,
    generation: u64,
) -> Result<(), SelectionError> {
    generation
        .checked_add(1)
        .ok_or(SelectionError::GenerationOverflow)?;
    settings.pending_update = Some(PendingSelectionUpdate {
        expected_generation: generation,
        selections: settings
            .selections
            .iter()
            .map(|item| Selection {
                id: item.id.clone(),
                source: item.source.clone(),
                active: item.active,
                root_kind: item.root_kind,
            })
            .collect(),
    });
    Ok(())
}

fn unique_id(settings: &ClientSettings) -> Result<String, SelectionError> {
    for _ in 0..8 {
        let id = random_id()?;
        if settings.selections.iter().all(|item| item.id != id) {
            return Ok(id);
        }
    }
    Err(SelectionError::IdExhausted)
}

pub(super) fn overlaps(left: &SourcePath, right: &SourcePath) -> bool {
    same_path(left, right) || strict_ancestor(left, right) || strict_ancestor(right, left)
}

fn strict_ancestor(parent: &SourcePath, child: &SourcePath) -> bool {
    if parent.drive() != child.drive() || parent.components().len() >= child.components().len() {
        return false;
    }
    let prefix = SourcePath::new(
        child.drive(),
        child.components()[..parent.components().len()].to_vec(),
    );
    prefix.is_ok_and(|prefix| same_path(parent, &prefix))
}
