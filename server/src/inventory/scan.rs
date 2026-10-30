use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use drip_core::{SourcePath, protocol::EntryKind};
use std::cmp::Ordering;

use crate::{
    StoreError, StoreResult, database,
    directory_scan::Budget,
    entry_files::{self, EntrySnapshot},
    upload_files, upload_paths,
};

use super::cursor::{CursorAnchor, source_path_bytes};

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
    after: Option<&CursorAnchor>,
) -> StoreResult<EntryPageScan> {
    let mut budget = Budget::default();
    let roots = super::roots::resolve(root, source, &mut budget)?;
    let mut scan = Scan {
        after,
        entries: Vec::with_capacity(PAGE_SIZE),
        has_more: false,
        anchor_found: false,
        budget,
    };
    if roots.is_empty() {
        if matches!(after, Some(CursorAnchor::Digest(_))) {
            return Err(StoreError::Conflict);
        }
        return Ok(EntryPageScan {
            entries: Vec::new(),
            has_more: false,
        });
    }
    for source in roots {
        let snapshot = entry_files::read_entry_with_budget(root, &source, &mut scan.budget)?
            .ok_or(StoreError::Conflict)?;
        let kind = snapshot.kind();
        scan.push(source.clone(), snapshot)?;
        if kind == EntryKind::Directory && !scan.has_more {
            let directory =
                entry_files::open_directory(root, &source)?.ok_or(StoreError::Conflict)?;
            scan.children(&source, &directory)?;
        }
        if scan.has_more {
            break;
        }
    }
    if matches!(after, Some(CursorAnchor::Digest(_))) && !scan.anchor_found {
        return Err(StoreError::Conflict);
    }
    Ok(EntryPageScan {
        entries: scan.entries,
        has_more: scan.has_more,
    })
}

struct Scan<'a> {
    after: Option<&'a CursorAnchor>,
    entries: Vec<ScannedEntry>,
    has_more: bool,
    anchor_found: bool,
    budget: Budget,
}

impl Scan<'_> {
    fn children(&mut self, source: &SourcePath, directory: &Dir) -> StoreResult<()> {
        let names = self.budget.names(directory, false)?;
        for name in names {
            if self.has_more {
                break;
            }
            let path = self.budget.child(source, &name)?;
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
                        snapshot: EntrySnapshot::Directory(
                            entry_files::snapshot_directory_with_budget(&child, &mut self.budget)?,
                        ),
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
        let bytes = source_path_bytes(source)?;
        match self.after {
            Some(CursorAnchor::Path(after))
                if compare_paths(source, after) != Ordering::Greater =>
            {
                return Ok(false);
            }
            Some(CursorAnchor::Digest(digest)) if !self.anchor_found => {
                if blake3::hash(&bytes).as_bytes() == digest {
                    self.anchor_found = true;
                }
                return Ok(false);
            }
            _ => {}
        }
        if self.entries.len() < PAGE_SIZE {
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
