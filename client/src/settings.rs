mod selection;
mod storage;
mod store;
mod validation;

use drip_core::{
    SourcePath,
    protocol::{EntryKind, Selection},
};
use serde::{Deserialize, Serialize};

pub use selection::{AddSelectionResult, SelectionError};
pub use store::{SettingsError, SettingsStore};

/// Settings independent of the authoritative DPAPI pairing envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub schema_version: u8,
    pub selections: Vec<SavedSelection>,
    pub acknowledged_generation: u64,
    pub pending_update: Option<PendingSelectionUpdate>,
    pub start_at_sign_in: bool,
    pub startup_executable: Option<String>,
    pub pc_display_name: Option<String>,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            selections: Vec::new(),
            acknowledged_generation: 0,
            pending_update: None,
            start_at_sign_in: false,
            startup_executable: None,
            pc_display_name: None,
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
