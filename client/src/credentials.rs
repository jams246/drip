use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

mod acl;
mod dpapi;
mod pairing;
mod storage;

pub use pairing::Pairing;

/// DPAPI-backed storage for `drip.credentials` beside the executable.
pub struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    /// Locates the credentials file beside the current executable.
    pub fn beside_executable() -> Result<Self, CredentialError> {
        let executable = std::env::current_exe().map_err(CredentialError::ExecutablePath)?;
        let directory = executable
            .parent()
            .ok_or(CredentialError::MissingExecutableDirectory)?;
        Ok(Self {
            path: directory.join("drip.credentials"),
        })
    }

    /// Loads and validates the current user's encrypted pairing envelope.
    pub fn load(&self) -> Result<Pairing, CredentialError> {
        let ciphertext = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CredentialError::NotPaired);
            }
            Err(source) => {
                return Err(CredentialError::Io {
                    action: "read",
                    path: self.path.clone(),
                    source,
                });
            }
        };
        let plaintext = dpapi::unprotect(&ciphertext).map_err(|_| CredentialError::Corrupt)?;
        Pairing::from_plaintext(&plaintext)
    }

    /// Atomically replaces the encrypted pairing envelope.
    pub fn save(&self, pairing: &Pairing) -> Result<(), CredentialError> {
        self.save_inner(pairing, |_| {})
    }

    fn save_inner(
        &self,
        pairing: &Pairing,
        checkpoint: impl FnMut(storage::Stage),
    ) -> Result<(), CredentialError> {
        let plaintext = pairing.to_plaintext()?;
        let ciphertext = dpapi::protect(&plaintext)?;
        storage::replace_with(&self.path, &ciphertext, checkpoint)
    }

    #[cfg(test)]
    fn save_with_checkpoint(
        &self,
        pairing: &Pairing,
        checkpoint: impl FnMut(storage::Stage),
    ) -> Result<(), CredentialError> {
        self.save_inner(pairing, checkpoint)
    }
}

/// Credential persistence failure with no secret values in its display text.
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("pairing is required; run `drip pair` first")]
    NotPaired,
    #[error("saved credentials are corrupt or belong to another Windows account; pair again")]
    Corrupt,
    #[error("invalid pairing: {0}")]
    InvalidPairing(&'static str),
    #[error("could not locate executable path: {0}")]
    ExecutablePath(#[source] io::Error),
    #[error("executable path has no parent directory")]
    MissingExecutableDirectory,
    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not encode pairing envelope: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("Windows DPAPI {operation} failed (code {code:#x})")]
    Dpapi { operation: &'static str, code: u32 },
    #[error("pairing envelope exceeds Windows DPAPI size limit")]
    DpapiTooLarge,
    #[error("Windows DPAPI returned an invalid {0} buffer")]
    DpapiOutput(&'static str),
    #[error("pairing file is not a regular file: {0}")]
    InvalidTarget(PathBuf),
    #[error("could not create a unique temporary credentials file beside {0}")]
    TemporaryNameExhausted(PathBuf),
}

pub(super) fn path_error(action: &'static str, path: &Path, source: io::Error) -> CredentialError {
    CredentialError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests;
