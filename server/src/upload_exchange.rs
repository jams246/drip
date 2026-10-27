use cap_std::fs::Dir;
use drip_core::SourcePath;

use crate::{
    StoreError, StoreResult, entry_files,
    entry_files::DirectorySnapshot,
    upload_files::{self, FileSnapshot, PublishFailure},
};

#[cfg(target_os = "linux")]
use crate::database;

pub(crate) struct ExchangePaths<'a> {
    pub(crate) staging: &'a Dir,
    pub(crate) stage_name: &'a str,
    pub(crate) parent: &'a Dir,
    pub(crate) filename: &'a str,
}

pub(crate) struct SelectedEntry<'a> {
    pub(crate) root: &'a Dir,
    pub(crate) source: &'a SourcePath,
}

pub(crate) fn probe(root: &Dir) -> StoreResult<bool> {
    #[cfg(target_os = "linux")]
    {
        probe_linux(root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
fn probe_linux(root: &Dir) -> StoreResult<bool> {
    use cap_fs_ext::DirExt;

    let staging = crate::upload_staging::open_dir(root)?;
    let id = crate::upload_store::random_upload_id()?;
    let name = format!(".exchange-probe-{id}");
    staging.create_dir(&name).map_err(database::io_error)?;
    let probe = match staging.open_dir_nofollow(&name) {
        Ok(probe) => probe,
        Err(error) => {
            let _ = staging.remove_dir(&name);
            return Err(database::io_error(error));
        }
    };
    let supported = probe_capability(&staging, &probe);
    probe.remove_open_dir_all().map_err(database::io_error)?;
    upload_files::fsync_directory(&staging)?;
    supported
}

#[cfg(target_os = "linux")]
fn probe_capability(staging: &Dir, probe: &Dir) -> StoreResult<bool> {
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
    use cap_std::fs::OpenOptions;
    use rustix::fs::{RenameFlags, renameat_with};

    (|| {
        probe.create_dir("directory").map_err(database::io_error)?;
        let nested = probe
            .open_dir_nofollow("directory")
            .map_err(database::io_error)?;
        let mut child_options = OpenOptions::new();
        child_options.write(true).create_new(true);
        child_options.follow(FollowSymlinks::No);
        drop(
            nested
                .open_with("child", &child_options)
                .map_err(database::io_error)?,
        );
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        options.follow(FollowSymlinks::No);
        drop(
            probe
                .open_with("file", &options)
                .map_err(database::io_error)?,
        );
        if let Err(error) = renameat_with(probe, "file", probe, "directory", RenameFlags::EXCHANGE)
        {
            return if exchange_is_unsupported(error) {
                Ok(false)
            } else {
                Err(database::io_error(error.into()))
            };
        }
        let file = probe.symlink_metadata("file").map_err(database::io_error)?;
        let directory = probe
            .symlink_metadata("directory")
            .map_err(database::io_error)?;
        if !file.is_dir() || !directory.is_file() {
            return Ok(false);
        }
        let exchanged = probe
            .open_dir_nofollow("file")
            .map_err(database::io_error)?;
        if !exchanged
            .symlink_metadata("child")
            .map_err(database::io_error)?
            .is_file()
        {
            return Ok(false);
        }
        upload_files::fsync_directory(probe)?;
        upload_files::fsync_directory(staging)?;
        Ok(true)
    })()
}

#[cfg(target_os = "linux")]
fn exchange_is_unsupported(error: rustix::io::Errno) -> bool {
    use rustix::io::Errno;

    matches!(error, Errno::INVAL | Errno::NOSYS | Errno::OPNOTSUPP)
}

pub(crate) fn replace_directory_with_file(
    paths: &ExchangePaths<'_>,
    target: &SelectedEntry<'_>,
    expected_directory: DirectorySnapshot,
    staged_file: &FileSnapshot,
) -> Result<(), PublishFailure> {
    upload_files::checkpoint("before_type_exchange");
    entry_files::check_directory(target.root, target.source, expected_directory)
        .map_err(|error| failure(error, false))?;
    exchange_checked(paths, || {
        upload_files::check_file_identity(paths.parent, paths.filename, staged_file)?;
        entry_files::check_directory_after_exchange(
            paths.staging,
            paths.stage_name,
            expected_directory,
        )
    })
}

pub(crate) fn replace_file_with_directory(
    paths: &ExchangePaths<'_>,
    target: &SelectedEntry<'_>,
    expected_file: &FileSnapshot,
    staged_directory: DirectorySnapshot,
) -> Result<(), PublishFailure> {
    upload_files::checkpoint("before_type_exchange");
    upload_files::check_snapshot(target.root, target.source, Some(expected_file))
        .map_err(|error| failure(error, false))?;
    exchange_checked(paths, || {
        upload_files::check_file_identity(paths.staging, paths.stage_name, expected_file)?;
        entry_files::check_directory_after_exchange(paths.parent, paths.filename, staged_directory)
    })
}

fn exchange_checked(
    paths: &ExchangePaths<'_>,
    verify: impl FnOnce() -> StoreResult<()>,
) -> Result<(), PublishFailure> {
    upload_files::fsync_directory(paths.staging).map_err(|error| failure(error, false))?;
    exchange_at(paths).map_err(|error| failure(error, false))?;
    upload_files::checkpoint("after_type_exchange");
    if let Err(error) = verify() {
        return match exchange_at(paths) {
            Ok(()) => {
                let _ = upload_files::fsync_directory(paths.parent);
                let _ = upload_files::fsync_directory(paths.staging);
                Err(failure(error, false))
            }
            Err(rollback_error) => Err(failure(rollback_error, true)),
        };
    }
    upload_files::fsync_directory(paths.parent).map_err(|error| failure(error, true))?;
    upload_files::fsync_directory(paths.staging).map_err(|error| failure(error, true))?;
    upload_files::checkpoint("after_type_exchange_sync");
    Ok(())
}

fn failure(error: StoreError, renamed: bool) -> PublishFailure {
    PublishFailure { error, renamed }
}

#[cfg(target_os = "linux")]
fn exchange_at(paths: &ExchangePaths<'_>) -> StoreResult<()> {
    use rustix::fs::{RenameFlags, renameat_with};

    renameat_with(
        paths.staging,
        paths.stage_name,
        paths.parent,
        paths.filename,
        RenameFlags::EXCHANGE,
    )
    .map_err(map_exchange_error)
}

#[cfg(target_os = "linux")]
fn map_exchange_error(error: rustix::io::Errno) -> StoreError {
    if exchange_is_unsupported(error) || error == rustix::io::Errno::XDEV {
        StoreError::UnsupportedExchange
    } else {
        crate::database::io_error(error.into())
    }
}

#[cfg(not(target_os = "linux"))]
fn exchange_at(_: &ExchangePaths<'_>) -> StoreResult<()> {
    Err(StoreError::Conflict)
}
