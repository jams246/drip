use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use drip_core::{SourcePath, protocol::EntryKind};
use std::cmp::Ordering;

use crate::{
    StoreError, StoreResult, database,
    entry_files::{self, EntrySnapshot},
    upload_files, upload_paths,
};

pub(crate) const PAGE_SIZE: usize = 1024;

pub(crate) struct ScannedEntry {
    pub(crate) source_path: SourcePath,
    pub(crate) snapshot: EntrySnapshot,
}

pub(crate) struct EntryPageScan {
    pub(crate) entries: Vec<ScannedEntry>,
    pub(crate) has_more: bool,
}

pub(crate) fn page(
    root: &Dir,
    source: &SourcePath,
    after: Option<&SourcePath>,
) -> StoreResult<EntryPageScan> {
    let mut scan = Scan {
        after,
        entries: Vec::with_capacity(PAGE_SIZE),
        has_more: false,
    };
    let Some(snapshot) = entry_files::read_entry(root, source)? else {
        return Ok(EntryPageScan {
            entries: Vec::new(),
            has_more: false,
        });
    };
    let kind = snapshot.kind();
    scan.push(source.clone(), snapshot)?;
    if kind == EntryKind::Directory && !scan.has_more {
        let directory = entry_files::open_directory(root, source)?.ok_or(StoreError::Conflict)?;
        scan.children(source, &directory)?;
    }
    Ok(EntryPageScan {
        entries: scan.entries,
        has_more: scan.has_more,
    })
}

struct Scan<'a> {
    after: Option<&'a SourcePath>,
    entries: Vec<ScannedEntry>,
    has_more: bool,
}

impl Scan<'_> {
    fn children(&mut self, source: &SourcePath, directory: &Dir) -> StoreResult<()> {
        let mut names = directory
            .read_dir(".")
            .map_err(database::io_error)?
            .map(|entry| entry.map_err(database::io_error))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .map(|entry| {
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| StoreError::UnsupportedEntry)
            })
            .collect::<StoreResult<Vec<_>>>()?;
        names.sort_unstable();
        for name in names {
            if self.has_more {
                break;
            }
            let path = child_path(source, &name).map_err(|_| StoreError::UnsupportedEntry)?;
            let metadata = match directory.symlink_metadata(&name) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(StoreError::Conflict);
                }
                Err(error) => return Err(database::io_error(error)),
            };
            let kind = if metadata.is_dir() {
                EntryKind::Directory
            } else if metadata.is_file() {
                EntryKind::File
            } else {
                return Err(StoreError::UnsupportedEntry);
            };
            let in_page = self.reserve_slot(&path)?;
            if self.has_more {
                break;
            }
            if kind == EntryKind::Directory {
                let child = directory
                    .open_dir_nofollow(&name)
                    .map_err(database::io_error)?;
                if in_page {
                    self.entries.push(ScannedEntry {
                        source_path: path.clone(),
                        snapshot: EntrySnapshot::Directory(entry_files::snapshot_directory(
                            &child,
                        )?),
                    });
                }
                self.children(&path, &child)?;
            } else if in_page {
                let file =
                    upload_paths::open_file(directory, &name)?.ok_or(StoreError::Conflict)?;
                self.entries.push(ScannedEntry {
                    source_path: path,
                    snapshot: EntrySnapshot::File(upload_files::hash_file(file)?),
                });
            }
        }
        Ok(())
    }

    fn push(&mut self, source_path: SourcePath, snapshot: EntrySnapshot) -> StoreResult<()> {
        if self.reserve_slot(&source_path)? {
            self.entries.push(ScannedEntry {
                source_path,
                snapshot,
            });
        }
        Ok(())
    }

    fn reserve_slot(&mut self, source: &SourcePath) -> StoreResult<bool> {
        if self
            .after
            .is_some_and(|after| compare_paths(source, after) != Ordering::Greater)
        {
            Ok(false)
        } else if self.entries.len() < PAGE_SIZE {
            super::validate_cursor_path(source)?;
            Ok(true)
        } else {
            self.has_more = true;
            Ok(false)
        }
    }
}

fn compare_paths(left: &SourcePath, right: &SourcePath) -> Ordering {
    left.drive()
        .cmp(&right.drive())
        .then_with(|| left.components().cmp(right.components()))
}

fn child_path(parent: &SourcePath, name: &str) -> Result<SourcePath, drip_core::SourcePathError> {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components)
}
