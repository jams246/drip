use serde::{Deserialize, Serialize};

use crate::{Blake3Digest, SourcePath, Timestamp};

/// Whole-file upload preparation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadPrepareRequest {
    /// Active selection authorizing this upload.
    pub selection_id: String,
    /// Current selection generation.
    pub generation: u64,
    /// Path within the selected source.
    pub source_path: SourcePath,
    /// Expected body length in bytes.
    pub expected_size: u64,
    /// Expected full-file BLAKE3 digest.
    pub digest: Blake3Digest,
    /// Source last-write time.
    pub last_write: Timestamp,
    /// Existing entry required for an atomic file/directory exchange.
    #[serde(default)]
    pub expected_entry: Option<EntryPrecondition>,
}

/// Opaque session and destination base returned by upload preparation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadPrepareResponse {
    /// Server-generated upload identifier.
    pub upload_id: String,
    /// Current destination revision, or `None` when absent.
    pub base_revision: Option<u64>,
}

/// Explicit upload session outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadState {
    /// Session accepts data or commit.
    Active,
    /// Verified file was published.
    Committed,
    /// Session cannot publish.
    Aborted,
}

/// Result returned by authenticated upload-state inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadStateResponse {
    /// Upload session identifier.
    pub upload_id: String,
    /// Current session outcome.
    pub state: UploadState,
}

/// Kind of actual mirror entry.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// Regular file.
    #[default]
    File,
    /// Directory, including an empty directory.
    Directory,
}

/// Current mirror entry identity required for a destructive replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryPrecondition {
    /// Entry kind observed during inventory.
    pub kind: EntryKind,
    /// Entry revision observed during inventory.
    pub revision: u64,
}

/// Actual mirror entry metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Source path represented by this entry.
    pub source_path: SourcePath,
    /// Filesystem entry kind.
    pub kind: EntryKind,
    /// File length in bytes, or zero for a directory.
    pub size: u64,
    /// Full-file digest for regular files.
    pub digest: Option<Blake3Digest>,
    /// Current destination revision.
    pub revision: u64,
    /// File last-write time where supported, absent for directories.
    pub last_write: Option<Timestamp>,
}

/// One paginated actual-mirror inventory response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryPage {
    /// Selection whose mirror entries are listed.
    pub selection_id: String,
    /// Entries in this page.
    pub entries: Vec<Entry>,
    /// Opaque cursor for the next page, if any.
    pub next_cursor: Option<String>,
}

/// Published metadata returned after upload commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadCommitResponse {
    /// Actual entry published by the server.
    pub entry: Entry,
}
