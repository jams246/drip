use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Unix timestamp with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Timestamp {
    unix_seconds: i64,
    nanos: u32,
}

impl Timestamp {
    /// Creates a timestamp when `nanos` is below one billion.
    pub fn new(unix_seconds: i64, nanos: u32) -> Result<Self, TimestampError> {
        if nanos >= 1_000_000_000 {
            return Err(TimestampError(nanos));
        }
        Ok(Self {
            unix_seconds,
            nanos,
        })
    }

    /// Returns whole seconds since the Unix epoch.
    pub const fn unix_seconds(self) -> i64 {
        self.unix_seconds
    }

    /// Returns the fractional nanoseconds.
    pub const fn nanos(self) -> u32 {
        self.nanos
    }

    /// Returns whether `stored` is this timestamp as kept by storage that may
    /// drop the fractional second, such as SFTP-backed mirrors.
    pub fn matches_stored(self, stored: Self) -> bool {
        stored == self || (stored.nanos == 0 && stored.unix_seconds == self.unix_seconds)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireTimestamp {
            unix_seconds: i64,
            nanos: u32,
        }

        let value = WireTimestamp::deserialize(deserializer)?;
        Self::new(value.unix_seconds, value.nanos).map_err(serde::de::Error::custom)
    }
}

/// Timestamp nanoseconds outside the protocol range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("timestamp nanoseconds must be below 1000000000, got {0}")]
pub struct TimestampError(pub u32);
