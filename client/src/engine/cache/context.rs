use drip_core::{Blake3Digest, SourcePath, Timestamp};

use crate::{settings::SavedSelection, source::Observation};

use super::database::CacheRecord;

pub(super) fn active_selections(selections: &[SavedSelection]) -> Vec<SavedSelection> {
    selections
        .iter()
        .filter(|selection| selection.active)
        .cloned()
        .collect()
}

pub(super) fn verified_record(
    path: &SourcePath,
    observation: Observation,
    volume_identity: &str,
    digest: Blake3Digest,
) -> Result<CacheRecord, String> {
    let verified_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("could not timestamp verified file contents: {error}"))?;
    let seconds = i64::try_from(verified_at.as_secs())
        .map_err(|error| format!("verified file timestamp is out of range: {error}"))?;
    let verified_at = Timestamp::new(seconds, verified_at.subsec_nanos())
        .map_err(|error| format!("verified file timestamp is invalid: {error}"))?;
    Ok(CacheRecord {
        path: path.clone(),
        observation,
        volume_identity: volume_identity.to_owned(),
        digest,
        verified_at,
    })
}
