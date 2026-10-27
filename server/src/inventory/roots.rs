use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use drip_core::SourcePath;

use crate::{StoreError, StoreResult, database};

pub(crate) fn resolve(root: &Dir, source: &SourcePath) -> StoreResult<Vec<SourcePath>> {
    let drive = source.drive().to_string();
    let drive = match root.open_dir_nofollow(&drive) {
        Ok(drive) => drive,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(StoreError::Conflict),
    };
    if source.components().is_empty() {
        return Ok(vec![source.clone()]);
    }
    let expected = source
        .components()
        .iter()
        .enumerate()
        .map(|(index, _)| {
            SourcePath::new(source.drive(), source.components()[..=index].to_vec())
                .map_err(|_| StoreError::Conflict)
        })
        .collect::<StoreResult<Vec<_>>>()?;
    let mut directories = vec![(drive, Vec::new())];

    for (index, prefix) in expected.iter().enumerate() {
        let final_component = index + 1 == expected.len();
        let mut next = Vec::new();
        let mut roots = Vec::new();
        for (directory, components) in directories {
            for name in names(&directory)? {
                let mut actual_components = components.clone();
                actual_components.push(name.clone());
                let Ok(actual) = SourcePath::new(source.drive(), actual_components.clone()) else {
                    continue;
                };
                if !actual.has_windows_prefix(prefix) {
                    continue;
                }
                let metadata = match directory.symlink_metadata(&name) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Err(StoreError::Conflict);
                    }
                    Err(error) => return Err(database::io_error(error)),
                };
                if final_component {
                    roots.push(actual);
                } else {
                    if !metadata.is_dir() {
                        return Err(StoreError::Conflict);
                    }
                    let child = directory
                        .open_dir_nofollow(&name)
                        .map_err(|_| StoreError::Conflict)?;
                    next.push((child, actual_components));
                }
            }
        }
        if final_component {
            roots.sort_unstable_by(|left, right| left.components().cmp(right.components()));
            return Ok(roots);
        }
        if next.is_empty() {
            return Ok(Vec::new());
        }
        directories = next;
    }
    Ok(Vec::new())
}

fn names(directory: &Dir) -> StoreResult<Vec<String>> {
    let mut names = directory
        .read_dir(".")
        .map_err(database::io_error)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name().into_string().ok())
                .map_err(database::io_error)
        })
        .collect::<StoreResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    names.sort_unstable();
    Ok(names)
}
