use axum::{http::HeaderMap, http::header::AUTHORIZATION};
use rusqlite::{OptionalExtension, Transaction};

use crate::{DeviceIdentity, StoreError, StoreResult, database};

pub(crate) fn credential(headers: &HeaderMap) -> Result<&str, StoreError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let Some(secret) = value.and_then(|value| value.strip_prefix("Bearer ")) else {
        return Err(StoreError::InvalidCredential);
    };
    if !is_lower_hex(secret, 64) {
        return Err(StoreError::InvalidCredential);
    }
    Ok(secret)
}

pub(crate) fn authenticate_transaction(
    transaction: &Transaction<'_>,
    credential: &str,
) -> StoreResult<DeviceIdentity> {
    authenticate_on(transaction, credential)
}

fn authenticate_on(
    connection: &rusqlite::Connection,
    credential: &str,
) -> StoreResult<DeviceIdentity> {
    if !is_lower_hex(credential, 64) {
        return Err(StoreError::InvalidCredential);
    }
    let digest = blake3::hash(credential.as_bytes()).to_hex().to_string();
    connection
        .query_row(
            "SELECT pc_id, mirror_directory FROM devices \
             WHERE credential_digest = ?1 AND revoked_at IS NULL",
            [&digest],
            |row| {
                Ok(DeviceIdentity {
                    pc_id: row.get(0)?,
                    mirror_directory: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(database::sql_error)?
        .ok_or(StoreError::InvalidCredential)
}

pub(crate) fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::is_lower_hex;

    #[test]
    fn lower_hex_requires_exact_lowercase_length() {
        assert!(is_lower_hex("0123abcdef", 10));
        assert!(!is_lower_hex("0123ABCDEF", 10));
        assert!(!is_lower_hex("0123abcde", 10));
    }
}
