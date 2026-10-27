use std::time::{Duration, SystemTime, UNIX_EPOCH};

use drip_core::Timestamp;

use crate::{StoreError, StoreResult};

pub(crate) fn metadata_timestamp(metadata: &cap_std::fs::Metadata) -> StoreResult<Timestamp> {
    system_time_timestamp(
        metadata
            .modified()
            .map_err(crate::database::io_error)?
            .into_std(),
    )
}

pub(crate) fn system_time_timestamp(time: SystemTime) -> StoreResult<Timestamp> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => Timestamp::new(
            i64::try_from(duration.as_secs()).map_err(|_| StoreError::Conflict)?,
            duration.subsec_nanos(),
        )
        .map_err(|_| StoreError::Conflict),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).map_err(|_| StoreError::Conflict)?;
            if duration.subsec_nanos() == 0 {
                Timestamp::new(-seconds, 0).map_err(|_| StoreError::Conflict)
            } else {
                Timestamp::new(-seconds - 1, 1_000_000_000 - duration.subsec_nanos())
                    .map_err(|_| StoreError::Conflict)
            }
        }
    }
}

pub(crate) fn timestamp_system_time(timestamp: Timestamp) -> StoreResult<SystemTime> {
    let nanos = timestamp.nanos();
    if timestamp.unix_seconds() >= 0 {
        UNIX_EPOCH
            .checked_add(Duration::new(timestamp.unix_seconds() as u64, nanos))
            .ok_or(StoreError::Conflict)
    } else if nanos == 0 {
        UNIX_EPOCH
            .checked_sub(Duration::from_secs(timestamp.unix_seconds().unsigned_abs()))
            .ok_or(StoreError::Conflict)
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::new(
                timestamp.unix_seconds().unsigned_abs() - 1,
                1_000_000_000 - nanos,
            ))
            .ok_or(StoreError::Conflict)
    }
}
