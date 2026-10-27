use std::io;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use drip_core::SourcePath;

use crate::{StoreError, StoreResult};

pub(crate) fn open_parent(
    root: &Dir,
    source: &SourcePath,
    create: bool,
) -> StoreResult<Option<(Dir, String)>> {
    let Some((filename, ancestors)) = source.components().split_last() else {
        return Err(StoreError::InvalidInput);
    };
    let Some(mut directory) = child_dir(root, &source.drive().to_string(), create)? else {
        return Ok(None);
    };
    for component in ancestors {
        let Some(child) = child_dir(&directory, component, create)? else {
            return Ok(None);
        };
        directory = child;
    }
    Ok(Some((directory, filename.clone())))
}

pub(crate) fn create_parent(root: &Dir, source: &SourcePath) -> StoreResult<(Dir, String)> {
    open_parent(root, source, true)?.ok_or(StoreError::InvalidInput)
}

pub(crate) fn open_file(parent: &Dir, name: &str) -> StoreResult<Option<File>> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.follow(FollowSymlinks::No);
    match parent.open_with(name, &options) {
        Ok(file) => {
            if file
                .metadata()
                .map_err(crate::database::io_error)?
                .is_file()
            {
                Ok(Some(file))
            } else {
                Err(StoreError::Conflict)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(StoreError::Conflict),
    }
}

fn child_dir(parent: &Dir, name: &str, create: bool) -> StoreResult<Option<Dir>> {
    if create {
        match parent.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(crate::database::io_error(error)),
        }
    }
    match parent.open_dir_nofollow(name) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create => Ok(None),
        Err(_) => Err(StoreError::Conflict),
    }
}
