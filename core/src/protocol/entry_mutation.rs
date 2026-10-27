use serde::{Deserialize, Serialize};

use super::{Entry, EntryPrecondition};
use crate::SourcePath;

/// Creates one directory inside an active selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCreateRequest {
    /// Active selection authorizing this directory.
    pub selection_id: String,
    /// Current selection generation.
    pub generation: u64,
    /// Path within the selected source.
    pub source_path: SourcePath,
    /// Existing file required for a type exchange, absent for ordinary creation.
    #[serde(default)]
    pub expected_entry: Option<EntryPrecondition>,
}

/// Directory published by a creation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCreateResponse {
    /// Actual directory entry after publication.
    pub entry: Entry,
}

/// Deletes one entry after a fresh revision and type check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDeleteRequest {
    /// Active selection authorizing this deletion.
    pub selection_id: String,
    /// Current selection generation.
    pub generation: u64,
    /// Path within the selected source.
    pub source_path: SourcePath,
    /// Entry observed during the latest actual-mirror inventory.
    pub expected_entry: EntryPrecondition,
}

/// Result of an idempotent single-entry deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDeleteResponse {
    /// Whether an entry existed and was removed.
    pub deleted: bool,
}
