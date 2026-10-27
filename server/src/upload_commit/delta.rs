use rusqlite::Transaction;

use crate::{Store, StoreError, StoreResult, upload_delta, upload_sessions::UploadSession};

pub(super) fn check_before_commit(
    store: &Store,
    upload_id: &str,
    expected_size: u64,
    enabled: bool,
) -> StoreResult<()> {
    if enabled {
        upload_delta::require_complete(store, upload_id, expected_size)?;
    }
    Ok(())
}

pub(super) fn require_complete(
    transaction: &Transaction<'_>,
    session: &UploadSession,
) -> StoreResult<()> {
    if session.delta_mode {
        upload_delta::require_complete_transaction(
            transaction,
            &session.id,
            session.expected_size,
        )?;
    } else if !session.has_data {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub(super) fn record_manifest(
    transaction: &Transaction<'_>,
    session: &UploadSession,
    revision: u64,
) -> StoreResult<()> {
    if session.delta_mode {
        upload_delta::record_committed_manifest(
            transaction,
            &session.pc_id,
            &session.source_path,
            revision,
            session.expected_size,
            session.digest,
            &session.id,
        )?;
    }
    Ok(())
}
