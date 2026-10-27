use std::io;

use cap_std::fs::Dir;
use drip_core::protocol::{Selection, SelectionRootMove};

use crate::{StoreError, StoreResult, database, entry_files, upload_files};

pub(crate) fn relocate(
    root: &Dir,
    previous: &[Selection],
    incoming: &[Selection],
    moves: &[SelectionRootMove],
) -> StoreResult<()> {
    for (index, moved) in moves.iter().enumerate() {
        if moves[..index]
            .iter()
            .any(|other| other.selection_id == moved.selection_id)
        {
            return Err(StoreError::InvalidInput);
        }
        let old = previous
            .iter()
            .find(|item| item.id == moved.selection_id)
            .ok_or(StoreError::NotFound)?;
        let new = incoming
            .iter()
            .find(|item| item.id == moved.selection_id)
            .ok_or(StoreError::Conflict)?;
        if !new.active
            || old.source != moved.previous_source
            || old.source == new.source
            || old.source.drive() != new.source.drive()
            || old.source.components().len() != new.source.components().len()
            || !old.source.has_windows_prefix(&new.source)
            || !new.source.has_windows_prefix(&old.source)
        {
            return Err(StoreError::Conflict);
        }
    }
    for moved in moves {
        let new = incoming
            .iter()
            .find(|item| item.id == moved.selection_id)
            .ok_or(StoreError::Conflict)?;
        move_root(root, &moved.previous_source, &new.source)?;
    }
    Ok(())
}

fn move_root(
    root: &Dir,
    old: &drip_core::SourcePath,
    new: &drip_core::SourcePath,
) -> StoreResult<()> {
    let old_parent = entry_files::open_parent(root, old)?;
    let (new_parent, new_name) = entry_files::create_parent(root, new)?;
    let old_exists = match &old_parent {
        Some((parent, name)) => ordinary_entry_exists(parent, name)?,
        None => false,
    };
    let new_exists = ordinary_entry_exists(&new_parent, &new_name)?;
    if old_exists && !new_exists {
        let (parent, name) = old_parent.as_ref().ok_or(StoreError::Conflict)?;
        upload_files::checkpoint("before_root_relocation");
        rename_noreplace(parent, name, &new_parent, &new_name)?;
        upload_files::checkpoint("after_root_relocation");
    }
    // A previous process may have moved the entry before its SQLite transaction committed.
    upload_files::fsync_directory(&new_parent)?;
    if let Some((parent, _)) = old_parent {
        upload_files::fsync_directory(&parent)?;
    }
    Ok(())
}

fn ordinary_entry_exists(parent: &Dir, name: &str) -> StoreResult<bool> {
    match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => Ok(true),
        Ok(_) => Err(StoreError::Conflict),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(database::io_error(error)),
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(old_parent: &Dir, old: &str, new_parent: &Dir, new: &str) -> StoreResult<()> {
    rustix::fs::renameat_with(
        old_parent,
        old,
        new_parent,
        new,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            StoreError::Conflict
        } else {
            database::io_error(error.into())
        }
    })
}

#[cfg(not(target_os = "linux"))]
fn rename_noreplace(_: &Dir, _: &str, _: &Dir, _: &str) -> StoreResult<()> {
    Err(StoreError::Conflict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_move_cannot_rebind_an_unrelated_root() {
        let fixture = crate::upload_race_test_common::Fixture::new().await;
        let current = fixture.store.get_selections(&fixture.credential).unwrap();
        let previous = current.selections;
        let mut incoming = previous.clone();
        incoming[0].source = drip_core::SourcePath::new('C', vec!["unrelated.bin".into()]).unwrap();
        let device =
            crate::upload_store::authenticated_device(&fixture.store, &fixture.credential).unwrap();
        let directory = fixture.store.open_device_directory(&device).unwrap();
        let moved = SelectionRootMove {
            selection_id: previous[0].id.clone(),
            previous_source: previous[0].source.clone(),
        };
        assert!(matches!(
            relocate(&directory, &previous, &incoming, &[moved]),
            Err(StoreError::Conflict)
        ));
        crate::upload_race_test_common::assert_old_bytes(&fixture);
        assert_eq!(
            fixture
                .store
                .get_selections(&fixture.credential)
                .unwrap()
                .selections,
            previous
        );
    }
}
