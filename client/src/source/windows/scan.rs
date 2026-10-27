mod walk;

use std::path::{Path, PathBuf};

use super::{
    error::SourceError,
    file::SourceFile,
    root::{self, SourceRoot},
};
use crate::settings::SavedSelection;
use drip_core::{SourcePath, Timestamp, protocol::EntryKind};
use walk::walk_directory;

/// One readable source entry from a recursive scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScannedEntry {
    pub source_path: SourcePath,
    pub local_path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
    pub last_write: Option<Timestamp>,
}

/// A path-specific skip or uncertainty that removes deletion authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanIssue {
    pub source_path: Option<SourcePath>,
    pub message: String,
}

/// Readable entries plus explicit completeness and protected application paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceScan {
    pub root_kind: EntryKind,
    pub complete: bool,
    pub entries: Vec<ScannedEntry>,
    pub issues: Vec<ScanIssue>,
    pub protected_paths: Vec<SourcePath>,
}

/// Scans a selected file or directory without traversing reparse points.
pub fn scan_root(selection: &SavedSelection, exe_dir: &Path) -> Result<SourceScan, SourceError> {
    let app_dir = root::capture_app_directory(exe_dir)?;
    let root_path = PathBuf::from(selection.source.to_windows_path());
    let current = root::capture_root(&root_path)?;
    if !same_root(selection, &current) {
        return Err(if current.volume_identity != selection.volume_identity {
            SourceError::VolumeChanged
        } else {
            SourceError::Changed
        });
    }
    let mut scan = SourceScan {
        root_kind: current.kind,
        complete: current.kind == selection.root_kind,
        entries: Vec::new(),
        issues: Vec::new(),
        protected_paths: Vec::new(),
    };
    if current.kind != selection.root_kind {
        scan.issue(
            Some(selection.source.clone()),
            "selected root kind changed; update selection generation",
        );
    }
    match current.kind {
        EntryKind::File => {
            let _ancestors =
                root::open_directory_chain(&selection.source, false, &selection.volume_identity)?;
            scan_file(selection, &root_path, &selection.source, &mut scan)?;
        }
        EntryKind::Directory => {
            scan.entries.push(ScannedEntry {
                source_path: selection.source.clone(),
                local_path: root_path.clone(),
                kind: EntryKind::Directory,
                size: 0,
                last_write: None,
            });
            walk_directory(
                selection,
                &app_dir,
                &root_path,
                &selection.source,
                &mut scan,
            )?;
        }
    }
    match root::capture_root(&root_path) {
        Ok(after) if same_root(selection, &after) && after.kind == current.kind => {}
        Ok(_) => scan.issue(
            Some(selection.source.clone()),
            "selected root changed during scan",
        ),
        Err(error) => scan.issue(Some(selection.source.clone()), error.to_string()),
    }
    Ok(scan)
}

fn scan_file(
    selection: &SavedSelection,
    path: &Path,
    source_path: &SourcePath,
    scan: &mut SourceScan,
) -> Result<(), SourceError> {
    match SourceFile::open(path) {
        Ok(file) if file.volume_identity() == selection.volume_identity => {
            if file.protocol_path() != source_path {
                scan.issue(
                    Some(source_path.clone()),
                    "opened file resolved to a different source path",
                );
            } else {
                scan.entries.push(ScannedEntry {
                    source_path: source_path.clone(),
                    local_path: path.to_owned(),
                    kind: EntryKind::File,
                    size: file.size(),
                    last_write: Some(file.last_write()),
                });
            }
        }
        Ok(_) => scan.issue(
            Some(source_path.clone()),
            SourceError::VolumeChanged.to_string(),
        ),
        Err(error) => scan.issue(Some(source_path.clone()), error.to_string()),
    }
    Ok(())
}

fn same_root(selection: &SavedSelection, current: &SourceRoot) -> bool {
    selection.source == current.source && selection.volume_identity == current.volume_identity
}

impl SourceScan {
    pub(super) fn issue(&mut self, path: Option<SourcePath>, message: impl Into<String>) {
        self.complete = false;
        self.issues.push(ScanIssue {
            source_path: path,
            message: message.into(),
        });
    }
}
