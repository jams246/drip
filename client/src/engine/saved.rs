use std::path::Path;

use drip_core::protocol::{
    SelectionRelocationRequest, SelectionRootMove, SelectionUpdate, SelectionUpdateResponse,
};
use reqwest::Method;

use crate::{
    credentials::Pairing,
    settings::{AddSelectionResult, ClientSettings, SettingsStore},
    source::capture_root,
    transport::HttpTransport,
};

use super::{
    Engine, EngineError, api,
    selection::{self, ActiveSelection},
};

impl Engine {
    /// Saves a selection locally, including its native volume identity.
    pub fn add(&self, path: &Path) -> Result<AddSelectionResult, EngineError> {
        Ok(self.settings.add(capture_root(path)?)?)
    }

    /// Saves removal, then waits for the server's mutation barrier.
    /// A transport failure leaves the removal pending for reconnect.
    pub async fn remove(&self, path: &Path) -> Result<(), EngineError> {
        let source = drip_core::SourcePath::try_from_path(path)
            .map_err(|_| crate::source::SourceError::InvalidPath)?;
        self.settings.remove(&source)?;
        let pairing = self.credentials.load()?;
        let transport = HttpTransport::new(pairing.server_url())?;
        synchronize(&self.settings, &transport, &pairing, true).await?;
        Ok(())
    }

    /// Returns saved selections and pending server-generation state.
    pub fn status(&self) -> Result<ClientSettings, EngineError> {
        Ok(self.settings.load()?)
    }

    /// Returns last reported runtime state, rebuilt from bounded local logs.
    pub fn snapshot(&self) -> Result<super::SyncSnapshot, EngineError> {
        Ok(super::ActivityLog::new(self.settings.directory()).snapshot()?)
    }
}

pub(super) async fn synchronize(
    store: &SettingsStore,
    transport: &HttpTransport,
    pairing: &Pairing,
    barrier: bool,
) -> Result<Vec<ActiveSelection>, EngineError> {
    let remote = selection::get(transport, pairing.credential()).await?;
    let local = store.load()?;
    if !barrier
        && local.pending_update.is_none()
        && local.acknowledged_generation == remote.generation
        && local.selections.iter().all(|local| {
            remote.selections.iter().any(|item| {
                item.id == local.id
                    && item.source == local.source
                    && item.active == local.active
                    && item.root_kind == local.root_kind
            })
        })
    {
        return Ok(active(local, remote.generation));
    }
    let pending = store.prepare_update(remote.generation)?;
    let mut root_moves = Vec::new();
    for incoming in &pending.selections {
        if let Some(previous) = remote
            .selections
            .iter()
            .find(|previous| previous.id == incoming.id)
            && previous.source != incoming.source
        {
            if !incoming.active || !crate::source::same_path(&previous.source, &incoming.source) {
                return Err(EngineError::SelectionChanged);
            }
            root_moves.push(SelectionRootMove {
                selection_id: incoming.id.clone(),
                previous_source: previous.source.clone(),
            });
        }
    }
    let update = SelectionUpdate {
        expected_generation: pending.expected_generation,
        selections: pending.selections.clone(),
    };
    let result: SelectionUpdateResponse = api::json(
        transport,
        Method::PUT,
        "v1/selections",
        Some(pairing.credential()),
        &[],
        Some(&SelectionRelocationRequest { update, root_moves }),
    )
    .await?;
    store.acknowledge_update(&pending, result.generation)?;
    let latest = store.load()?;
    if latest.pending_update.is_some() {
        return Err(EngineError::SelectionChanged);
    }
    Ok(active(latest, result.generation))
}

fn active(settings: ClientSettings, generation: u64) -> Vec<ActiveSelection> {
    settings
        .selections
        .into_iter()
        .filter(|item| item.active)
        .map(|item| ActiveSelection {
            selection: drip_core::protocol::Selection {
                id: item.id,
                source: item.source,
                root_kind: item.root_kind,
                active: true,
            },
            generation,
        })
        .collect()
}
