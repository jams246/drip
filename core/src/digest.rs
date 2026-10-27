use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Full 256-bit BLAKE3 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Blake3Digest([u8; 32]);

impl Blake3Digest {
    /// Creates a digest from its 32 bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses exactly 64 lowercase hexadecimal digits.
    pub fn from_hex(value: &str) -> Result<Self, DigestError> {
        if value.len() != 64 {
            return Err(DigestError);
        }

        let mut bytes = [0; 32];
        let (pairs, _) = value.as_bytes().as_chunks::<2>();
        for (index, pair) in pairs.iter().enumerate() {
            bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }

    /// Returns the digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns 64 lowercase hexadecimal digits.
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

impl Serialize for Blake3Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Blake3Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_hex(&value).map_err(serde::de::Error::custom)
    }
}

fn hex_nibble(byte: u8) -> Result<u8, DigestError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(DigestError),
    }
}

/// Digest was not 64 lowercase hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("digest must be 64 lowercase hexadecimal digits")]
pub struct DigestError;
