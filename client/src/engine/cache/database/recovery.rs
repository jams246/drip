use std::{
    fs::{self, File},
    io,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};

use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::DatabaseError;

pub fn preflight_database(path: &Path) -> Result<bool, DatabaseError> {
    let exists = preflight(path)?;
    for suffix in ["-journal", "-wal", "-shm"] {
        let artifact = sidecar(path, suffix)?;
        if preflight(&artifact)? {
            secure_existing_sidecar(&artifact)?;
        }
    }
    Ok(exists)
}

pub fn preflight(path: &Path) -> Result<bool, DatabaseError> {
    validate_parent(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(unsafe_path(path));
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DatabaseError::Recovery(format!(
            "could not inspect {}: {error}",
            path.display()
        ))),
    }
}

pub fn create_empty(path: &Path) -> Result<(), DatabaseError> {
    match crate::private_file::open(path, true, true) {
        Ok(file) => {
            file.sync_all().map_err(|error| {
                DatabaseError::Recovery(format!("could not sync {}: {error}", path.display()))
            })?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if preflight(path)? {
                secure_database(path)
            } else {
                Err(DatabaseError::Recovery(format!(
                    "{} disappeared during creation",
                    path.display()
                )))
            }
        }
        Err(error) => Err(DatabaseError::Recovery(format!(
            "could not create {}: {error}",
            path.display()
        ))),
    }
}

pub fn secure_database(path: &Path) -> Result<(), DatabaseError> {
    if !preflight(path)? {
        return Err(DatabaseError::Recovery(format!(
            "{} is missing",
            path.display()
        )));
    }
    crate::credentials::acl::secure_file(path).map_err(|error| {
        DatabaseError::Recovery(format!("could not protect {}: {error}", path.display()))
    })
}

pub fn open_guard(path: &Path) -> Result<File, DatabaseError> {
    if !preflight(path)? {
        return Err(DatabaseError::Recovery(format!(
            "{} is missing",
            path.display()
        )));
    }
    crate::private_file::open(path, true, false).map_err(|error| {
        DatabaseError::Recovery(format!("could not open {} safely: {error}", path.display()))
    })
}

pub fn prepare_journal(path: &Path) -> Result<(), DatabaseError> {
    let _ = preflight(path)?;
    let journal = sidecar(path, "-journal")?;
    match crate::private_file::open(&journal, true, true) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            secure_existing_sidecar(&journal)
        }
        Err(error) => Err(DatabaseError::Recovery(format!(
            "could not create {}: {error}",
            journal.display()
        ))),
    }
}

pub fn preserve(path: &Path) -> Result<(), DatabaseError> {
    let mut sources = Vec::new();
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let source = if suffix.is_empty() {
            path.to_owned()
        } else {
            sidecar(path, suffix)?
        };
        if preflight(&source)? {
            sources.push((source, suffix));
        }
    }
    if sources.is_empty() {
        return Ok(());
    }
    for _ in 0..32 {
        let id = crate::source::random_id().map_err(|error| {
            DatabaseError::Recovery(format!("could not create recovery ID: {error}"))
        })?;
        let targets: Vec<_> = sources
            .iter()
            .map(|(_, suffix)| {
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(format!("drip.state.corrupt.{id}.sqlite3{suffix}"))
            })
            .collect();
        if targets
            .iter()
            .any(|target| fs::symlink_metadata(target).is_ok())
        {
            continue;
        }
        for ((source, _), target) in sources.iter().zip(&targets) {
            fs::rename(source, target).map_err(|error| {
                DatabaseError::Recovery(format!(
                    "could not preserve {} as {}: {error}",
                    source.display(),
                    target.display()
                ))
            })?;
        }
        return Ok(());
    }
    Err(DatabaseError::Recovery(
        "could not allocate a unique corruption recovery set".into(),
    ))
}

fn secure_existing_sidecar(path: &Path) -> Result<(), DatabaseError> {
    if !preflight(path)? {
        return Err(unsafe_path(path));
    }
    crate::credentials::acl::secure_file(path).map_err(|error| {
        DatabaseError::Recovery(format!("could not protect {}: {error}", path.display()))
    })
}

fn validate_parent(path: &Path) -> Result<(), DatabaseError> {
    let Some(parent) = path.parent() else {
        return Err(unsafe_path(path));
    };
    let mut current: Option<&Path> = Some(parent);
    while let Some(directory) = current {
        let metadata = fs::symlink_metadata(directory).map_err(|error| {
            DatabaseError::Recovery(format!(
                "could not inspect database directory {}: {error}",
                directory.display()
            ))
        })?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(unsafe_path(directory));
        }
        current = directory.parent();
    }
    Ok(())
}

pub(super) fn hold_parents(path: &Path) -> Result<Vec<File>, DatabaseError> {
    crate::private_file::parents(path).map_err(|error| {
        DatabaseError::Recovery(format!("could not hold database directory safely: {error}"))
    })
}

fn sidecar(path: &Path, suffix: &str) -> Result<PathBuf, DatabaseError> {
    let name = path
        .file_name()
        .ok_or_else(|| unsafe_path(path))?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{name}{suffix}")))
}

fn unsafe_path(path: &Path) -> DatabaseError {
    DatabaseError::UnsafePath(path.display().to_string())
}
