use drip_core::SourcePath;

use crate::{StoreError, StoreResult};

const MAX_CURSOR_BYTES: usize = 4096;
pub(super) const MAX_PATH_RECORD_BYTES: usize = 256 * 1024;

pub(super) enum CursorAnchor {
    Path(SourcePath),
    Digest([u8; 32]),
}

pub(super) fn encode(source: &SourcePath) -> StoreResult<String> {
    let bytes = source_path_bytes(source)?;
    if bytes.len() <= MAX_CURSOR_BYTES / 2 {
        Ok(hex_encode(&bytes))
    } else {
        Ok(format!(
            "b3-{}",
            hex_encode(blake3::hash(&bytes).as_bytes())
        ))
    }
}

pub(super) fn decode(cursor: &str) -> StoreResult<CursorAnchor> {
    if cursor.len() > MAX_CURSOR_BYTES {
        return Err(StoreError::InvalidInput);
    }
    if let Some(encoded) = cursor.strip_prefix("b3-") {
        if encoded.len() != 64 {
            return Err(StoreError::InvalidInput);
        }
        let digest: [u8; 32] = hex_decode(encoded)?
            .try_into()
            .map_err(|_| StoreError::InvalidInput)?;
        return Ok(CursorAnchor::Digest(digest));
    }
    if cursor.is_empty() || !cursor.len().is_multiple_of(2) {
        return Err(StoreError::InvalidInput);
    }
    let bytes = hex_decode(cursor)?;
    serde_json::from_slice(&bytes)
        .map(CursorAnchor::Path)
        .map_err(|_| StoreError::InvalidInput)
}

pub(super) fn source_path_bytes(source: &SourcePath) -> StoreResult<Vec<u8>> {
    let bytes = serde_json::to_vec(source).map_err(StoreError::Serialization)?;
    if bytes.len() > MAX_PATH_RECORD_BYTES {
        Err(StoreError::UnsupportedEntry)
    } else {
        Ok(bytes)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn hex_decode(encoded: &str) -> StoreResult<Vec<u8>> {
    if !encoded.len().is_multiple_of(2) {
        return Err(StoreError::InvalidInput);
    }
    encoded
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_value(pair[0]).ok_or(StoreError::InvalidInput)?;
            let low = hex_value(pair[1]).ok_or(StoreError::InvalidInput)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PATH_RECORD_BYTES, encode};
    use crate::StoreError;
    use drip_core::SourcePath;

    #[test]
    fn cursor_hashes_long_paths_and_rejects_oversized_records() {
        let long = SourcePath::new('C', vec!["x".repeat(3000)]).unwrap();
        let cursor = encode(&long).unwrap();
        assert!(cursor.starts_with("b3-"));
        assert!(cursor.len() <= 4096);

        let oversized = SourcePath::new('C', vec!["x".repeat(MAX_PATH_RECORD_BYTES)]).unwrap();
        assert!(matches!(
            encode(&oversized),
            Err(StoreError::UnsupportedEntry)
        ));
    }
}
