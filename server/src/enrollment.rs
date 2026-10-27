use rusqlite::{OptionalExtension, TransactionBehavior};

use drip_core::{
    PROTOCOL_VERSION, ProtocolVersion, SensitiveString,
    protocol::{EnrollmentRequest, EnrollmentResponse},
};

use crate::{Store, StoreError, StoreResult, database, mirror};

const TOKEN_LIFETIME_SECONDS: i64 = 15 * 60;

impl Store {
    /// Creates a single-use, 15-minute enrollment token and returns it once.
    pub fn create_enrollment_token(&self) -> StoreResult<String> {
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database::sql_error)?;
        let token = random_hex(32)?;
        let digest = blake3::hash(token.as_bytes()).to_hex().to_string();
        let now = self.now()?;
        let expires_at = now
            .checked_add(TOKEN_LIFETIME_SECONDS)
            .ok_or(StoreError::Clock)?;
        transaction
            .execute(
                "INSERT INTO enrollment_tokens(digest, expires_at) VALUES (?1, ?2)",
                (&digest, expires_at),
            )
            .map_err(database::sql_error)?;
        transaction.commit().map_err(database::sql_error)?;
        Ok(token)
    }

    /// Atomically consumes a token, registers its device, and creates the isolated mirror directory.
    pub fn enroll(&self, request: EnrollmentRequest) -> StoreResult<EnrollmentResponse> {
        let token = request.token.expose_secret();
        let label = validate_label(&request.label)?;
        let token_digest = blake3::hash(token.as_bytes()).to_hex().to_string();
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database::sql_error)?;
        let now = self.now()?;
        let token_state = transaction
            .query_row(
                "SELECT expires_at, consumed_at FROM enrollment_tokens WHERE digest = ?1",
                [&token_digest],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()
            .map_err(database::sql_error)?;
        let Some((expires_at, consumed_at)) = token_state else {
            return Err(StoreError::InvalidCredential);
        };
        if consumed_at.is_some() || now >= expires_at {
            return Err(StoreError::InvalidCredential);
        }
        let consumed = transaction
            .execute(
                "UPDATE enrollment_tokens SET consumed_at = ?1 \
             WHERE digest = ?2 AND consumed_at IS NULL AND expires_at > ?1",
                (&now, &token_digest),
            )
            .map_err(database::sql_error)?;
        if consumed != 1 {
            return Err(StoreError::InvalidCredential);
        }

        let (pc_id, mirror_directory) = create_identity(&self.mirror, label)?;
        let credential = random_hex(32)?;
        let credential_digest = blake3::hash(credential.as_bytes()).to_hex().to_string();
        transaction.execute(
            "INSERT INTO devices(pc_id, label, mirror_directory, credential_digest, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (&pc_id, label, &mirror_directory, &credential_digest, &now),
        ).map_err(database::sql_error)?;
        transaction
            .execute(
                "INSERT INTO device_state(pc_id, generation) VALUES (?1, 0)",
                [&pc_id],
            )
            .map_err(database::sql_error)?;
        transaction.commit().map_err(database::sql_error)?;
        Ok(EnrollmentResponse {
            pc_id,
            mirror_directory,
            device_credential: SensitiveString::new(credential),
            protocol_version: ProtocolVersion::new(PROTOCOL_VERSION),
        })
    }
}

fn create_identity(mirror: &cap_std::fs::Dir, label: &str) -> StoreResult<(String, String)> {
    for _ in 0..4 {
        let pc_id = random_hex(16)?;
        let mirror_directory = format!("{}-{pc_id}", sanitize_label(label));
        if mirror::create_device_directory(mirror, &mirror_directory)? {
            return Ok((pc_id, mirror_directory));
        }
    }
    Err(StoreError::Conflict)
}

fn validate_label(label: &str) -> StoreResult<&str> {
    let label = label.trim();
    if label.is_empty() || label.len() > 128 || label.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput);
    }
    Ok(label)
}

fn sanitize_label(label: &str) -> String {
    let mut safe = String::with_capacity(48);
    for character in label.chars() {
        if safe.len() >= 48 {
            break;
        }
        if character.is_ascii_alphanumeric() {
            safe.push(character.to_ascii_lowercase());
        } else if !safe.is_empty() && !safe.ends_with('-') {
            safe.push('-');
        }
    }
    let safe = safe.trim_matches('-');
    if safe.is_empty() {
        "pc".to_owned()
    } else {
        safe.to_owned()
    }
}

fn random_hex(bytes_len: usize) -> StoreResult<String> {
    let mut bytes = vec![0; bytes_len];
    getrandom::fill(&mut bytes).map_err(|_| StoreError::Random)?;
    let mut hex = String::with_capacity(bytes_len * 2);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::{sanitize_label, validate_label};

    #[test]
    fn label_directory_component_is_sanitized_and_bounded() {
        assert_eq!(sanitize_label("Home Office!"), "home-office");
        assert!(sanitize_label(&"x".repeat(200)).len() <= 48);
    }

    #[test]
    fn blank_or_control_labels_are_rejected() {
        assert!(validate_label(" \n ").is_err());
        assert!(validate_label("desktop\u{7}").is_err());
    }
}
