use serde::{Deserialize, Serialize};

use super::CredentialError;
use crate::transport::ServerUrl;

/// One authoritative binding between a server and a registered PC.
#[derive(Serialize)]
pub struct Pairing {
    server_url: String,
    pc_id: String,
    mirror_directory: String,
    credential: String,
}

impl Pairing {
    /// Creates a validated pairing and stores the canonical HTTPS origin.
    pub fn new(
        server_url: &str,
        pc_id: String,
        mirror_directory: String,
        credential: String,
    ) -> Result<Self, CredentialError> {
        let server_url = ServerUrl::parse(server_url)
            .map_err(|_| {
                CredentialError::InvalidPairing("server URL must be a valid HTTPS origin")
            })?
            .as_str()
            .to_owned();
        validate_pairing(&pc_id, &mirror_directory, &credential)?;
        Ok(Self {
            server_url,
            pc_id,
            mirror_directory,
            credential,
        })
    }

    /// Returns the canonical HTTPS origin.
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Returns the server-assigned PC identity.
    pub fn pc_id(&self) -> &str {
        &self.pc_id
    }

    /// Returns the server-assigned mirror directory.
    pub fn mirror_directory(&self) -> &str {
        &self.mirror_directory
    }

    /// Returns the device credential for authenticated requests.
    pub fn credential(&self) -> &str {
        &self.credential
    }

    pub(super) fn to_plaintext(&self) -> Result<Vec<u8>, CredentialError> {
        serde_json::to_vec(self).map_err(CredentialError::Serialize)
    }

    pub(super) fn from_plaintext(plaintext: &[u8]) -> Result<Self, CredentialError> {
        let data: PairingData =
            serde_json::from_slice(plaintext).map_err(|_| CredentialError::Corrupt)?;
        Self::new(
            &data.server_url,
            data.pc_id,
            data.mirror_directory,
            data.credential,
        )
        .map_err(|_| CredentialError::Corrupt)
    }
}

#[derive(Deserialize)]
struct PairingData {
    server_url: String,
    pc_id: String,
    mirror_directory: String,
    credential: String,
}

fn validate_pairing(
    pc_id: &str,
    mirror_directory: &str,
    credential: &str,
) -> Result<(), CredentialError> {
    if pc_id.len() != 32
        || !pc_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CredentialError::InvalidPairing(
            "PC ID must be 32 lowercase hexadecimal characters",
        ));
    }
    if credential.len() != 64
        || !credential
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CredentialError::InvalidPairing(
            "device credential must be 64 lowercase hexadecimal characters",
        ));
    }
    let suffix = format!("-{pc_id}");
    let safe_component = !mirror_directory.is_empty()
        && mirror_directory.len() <= 255
        && mirror_directory != "."
        && mirror_directory != ".."
        && !mirror_directory.ends_with([' ', '.'])
        && mirror_directory.bytes().all(|byte| {
            byte.is_ascii_graphic()
                && !matches!(
                    byte,
                    b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
                )
        });
    if !safe_component || !mirror_directory.ends_with(&suffix) {
        return Err(CredentialError::InvalidPairing(
            "mirror directory must be a safe component ending in the PC ID",
        ));
    }
    Ok(())
}
