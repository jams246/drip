pub(crate) mod activity;
mod api;
mod cache;
pub(crate) mod connection;
mod error;
mod lifecycle;
mod logging;
mod mirror_api;
mod pass;
mod run;
mod saved;
mod schedule;
mod selection;
mod status;
#[cfg(test)]
pub(crate) mod test_metrics;
mod upload;
mod watcher;

pub use activity::{ActivityEvent, ProgressSnapshot, UploadPhase};
pub use error::EngineError;
pub use lifecycle::{EventKind, EventSink, LifecycleEvent, SelectionProgress};
pub use logging::ActivityLog;
pub use run::RunControl;
pub use status::{ConnectionState, RootStatus, SelectionState, SyncSnapshot};

use std::path::Path;

use drip_core::protocol::EnrollmentRequest;
use reqwest::Method;

use crate::{
    credentials::{CredentialStore, Pairing},
    source::SourceFile,
    transport::HttpTransport,
};

/// Shared pairing and one-file upload engine for the diagnostic CLI and GUI.
pub struct Engine {
    credentials: CredentialStore,
    settings: crate::settings::SettingsStore,
}

/// Metadata verified by the server after upload publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadResult {
    /// Original drive and component spelling.
    pub source_path: drip_core::SourcePath,
    /// Published file length.
    pub size: u64,
    /// Published full-file BLAKE3 digest.
    pub digest: drip_core::Blake3Digest,
}

impl Engine {
    /// Creates an engine using the executable's adjacent credential envelope.
    pub fn beside_executable() -> Result<Self, EngineError> {
        Ok(Self {
            credentials: CredentialStore::beside_executable()?,
            settings: crate::settings::SettingsStore::beside_executable()?,
        })
    }

    /// Exchanges a one-time token and saves the complete DPAPI binding.
    pub async fn pair(
        &self,
        server_url: &str,
        label: &str,
        token: &str,
    ) -> Result<Pairing, EngineError> {
        if !lower_hex(token, 64) {
            return Err(EngineError::InvalidEnrollmentToken);
        }
        let transport = HttpTransport::new(server_url)?;
        let request = EnrollmentRequest {
            token: drip_core::SensitiveString::new(token),
            label: label.to_owned(),
        };
        let response: drip_core::protocol::EnrollmentResponse = api::json(
            &transport,
            Method::POST,
            "v1/enroll",
            None,
            &[],
            Some(&request),
        )
        .await?;
        response
            .protocol_version
            .validate()
            .map_err(|error| EngineError::UnsupportedProtocol(error.0))?;
        let pairing = Pairing::new(
            transport.server().as_str(),
            response.pc_id,
            response.mirror_directory,
            response.device_credential.expose_secret().to_owned(),
        )?;
        self.credentials.save(&pairing)?;
        Ok(pairing)
    }

    /// Uploads one regular local file through the shared engine.
    pub async fn upload(
        &self,
        path: &Path,
        events: &tokio::sync::mpsc::Sender<ActivityEvent>,
    ) -> Result<UploadResult, EngineError> {
        let pairing = self.credentials.load()?;
        let transport = HttpTransport::new(pairing.server_url())?;
        let source = SourceFile::open(path)?;
        upload::upload(&transport, &pairing, source, events).await
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
