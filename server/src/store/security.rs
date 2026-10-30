use std::{io, path::Path};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};

use crate::{StoreResult, database};

pub(super) fn root(path: &Path) -> StoreResult<Dir> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(database::io_error)?;
    let metadata = std::fs::symlink_metadata(path).map_err(database::io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_storage());
    }
    let directory = crate::mirror::open_root(path)?;
    private_directory(&directory)?;
    Ok(directory)
}

pub(crate) fn private_directory(directory: &Dir) -> StoreResult<()> {
    #[cfg(target_os = "linux")]
    let file = {
        let mut options = OpenOptions::new();
        options.read(true);
        options.follow(FollowSymlinks::No);
        directory
            .open_with(".", &options)
            .map_err(database::io_error)?
            .into_std()
    };
    #[cfg(not(target_os = "linux"))]
    let file = directory
        .try_clone()
        .map_err(database::io_error)?
        .into_std_file();
    secure(&file, true)
}

pub(super) fn database_files(directory: &Dir) -> StoreResult<()> {
    drop(private_file(directory, "drip.sqlite3", true)?);
    for name in [
        "drip.sqlite3-wal",
        "drip.sqlite3-shm",
        "drip.sqlite3-journal",
    ] {
        drop(private_file(directory, name, false)?);
    }
    Ok(())
}

pub(super) fn private_file(
    directory: &Dir,
    name: &str,
    create: bool,
) -> StoreResult<Option<std::fs::File>> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
    options.follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
        #[cfg(target_os = "linux")]
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = match directory.open_with(name, &options) {
        Ok(file) => file.into_std(),
        Err(error) if !create && error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(database::io_error(error)),
    };
    secure(&file, false)?;
    Ok(Some(file))
}

fn secure(file: &std::fs::File, directory: bool) -> StoreResult<()> {
    let metadata = file.metadata().map_err(database::io_error)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(invalid_storage());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SQLite may unlink a sidecar after it has been opened for validation.
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || (!directory && metadata.nlink() > 1)
        {
            return Err(invalid_storage());
        }
        let mode = if directory { 0o700 } else { 0o600 };
        if metadata.mode() & 0o7777 != mode {
            file.set_permissions(std::fs::Permissions::from_mode(mode))
                .map_err(database::io_error)?;
        }
    }
    Ok(())
}

fn invalid_storage() -> crate::StoreError {
    database::io_error(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "server storage must use private, service-owned ordinary files and directories",
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[tokio::test]
    async fn removed_sidecar_does_not_fail_storage_validation() {
        let fixture = crate::upload_race_test_common::Fixture::new().await;
        let directory = &fixture.store.state_directory;
        let file = super::private_file(directory, "synthetic-sidecar", true)
            .unwrap()
            .unwrap();
        directory.remove_file("synthetic-sidecar").unwrap();
        assert!(super::secure(&file, false).is_ok());
    }
}
