use serde::{Deserialize, Serialize};

use super::EntryKind;
use crate::SourcePath;

/// One source path and its active or retained state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// Opaque selection identifier scoped to the authenticated device.
    pub id: String,
    /// Validated absolute source path.
    pub source: SourcePath,
    /// Selected root type and descendant mutation authority.
    #[serde(default)]
    pub root_kind: EntryKind,
    /// Whether this selection currently accepts mutations.
    pub active: bool,
}

/// Current selection records and mutation generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionSet {
    /// Current generation.
    pub generation: u64,
    /// Active and retained inactive selections.
    pub selections: Vec<Selection>,
}

/// Selection update guarded by the generation read by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionUpdate {
    /// Generation precondition for the update.
    pub expected_generation: u64,
    /// Complete selection state to publish.
    pub selections: Vec<Selection>,
}

/// New generation returned after a selection update barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionUpdateResponse {
    /// Generation established by the server.
    pub generation: u64,
}
