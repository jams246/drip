use std::io;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};

use crate::{StoreError, StoreResult};

const STAGING_NAME: &str = ".drip-staging";

pub(crate) fn create(root: &Dir, id: &str) -> StoreResult<String> {
    let directory = ensure(root)?;
    let name = format!("{id}.part");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options.follow(FollowSymlinks::No);
    let _file = directory.open_with(&name, &options).map_err(map_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = _file.set_permissions(cap_std::fs::Permissions::from_std(
            std::fs::Permissions::from_mode(0o600),
        )) {
            drop(_file);
            let _ = directory.remove_file(&name);
            return Err(map_error(error));
        }
    }
    Ok(name)
}

pub(crate) fn open_write(root: &Dir, name: &str) -> StoreResult<File> {
    let directory = ensure(root)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    options.follow(FollowSymlinks::No);
    directory.open_with(name, &options).map_err(map_error)
}

pub(crate) fn open_dir(root: &Dir) -> StoreResult<Dir> {
    ensure(root)
}

pub(crate) fn remove(root: &Dir, name: &str) -> StoreResult<()> {
    match ensure(root)?.remove_file(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

pub(crate) fn clean(root: &Dir) -> StoreResult<()> {
    match root.open_dir_nofollow(STAGING_NAME) {
        Ok(directory) => directory
            .remove_open_dir_all()
            .map_err(crate::database::io_error)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(crate::database::io_error(error)),
    }
    ensure(root).map(|_| ())
}

fn ensure(root: &Dir) -> StoreResult<Dir> {
    match root.create_dir(STAGING_NAME) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                root.set_permissions(
                    STAGING_NAME,
                    cap_std::fs::Permissions::from_std(std::fs::Permissions::from_mode(0o700)),
                )
                .map_err(crate::database::io_error)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(crate::database::io_error(error)),
    }
    root.open_dir_nofollow(STAGING_NAME)
        .map_err(crate::database::io_error)
}

fn map_error(error: io::Error) -> StoreError {
    if error.kind() == io::ErrorKind::StorageFull {
        StoreError::InsufficientStagingSpace
    } else if error.kind() == io::ErrorKind::AlreadyExists {
        StoreError::Conflict
    } else {
        crate::database::io_error(error)
    }
}
