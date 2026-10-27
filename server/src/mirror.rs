use std::path::Path;

use cap_std::{ambient_authority, fs::Dir};

use crate::{StoreError, StoreResult, database};

pub(crate) fn open_root(path: &Path) -> StoreResult<Dir> {
    Dir::open_ambient_dir(path, ambient_authority()).map_err(database::io_error)
}

pub(crate) fn create_device_directory(root: &Dir, name: &str) -> StoreResult<bool> {
    match root.create_dir(name) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                root.set_permissions(
                    name,
                    cap_std::fs::Permissions::from_std(std::fs::Permissions::from_mode(0o700)),
                )
                .map_err(database::io_error)?;
            }
            crate::upload_files::fsync_directory(root)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(StoreError::Filesystem(error)),
    }
}
