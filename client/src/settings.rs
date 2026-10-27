mod selection;
mod storage;
mod store;

use drip_core::{
    SourcePath,
    protocol::{EntryKind, Selection},
};
use serde::{Deserialize, Serialize};

pub use selection::AddSelectionResult;
pub use store::{SettingsError, SettingsStore};

/// Settings independent of the authoritative DPAPI pairing envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub schema_version: u8,
    pub selections: Vec<SavedSelection>,
    pub acknowledged_generation: u64,
    pub pending_update: Option<PendingSelectionUpdate>,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            selections: Vec::new(),
            acknowledged_generation: 0,
            pending_update: None,
        }
    }
}

/// Locally retained selection and source-volume identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedSelection {
    pub id: String,
    pub source: SourcePath,
    pub root_kind: EntryKind,
    pub volume_identity: String,
    pub active: bool,
}

/// Selection update persisted until the server acknowledges its generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingSelectionUpdate {
    pub expected_generation: u64,
    pub selections: Vec<Selection>,
}
