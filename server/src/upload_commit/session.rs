use crate::{Store, StoreResult, upload_sessions::UploadSession};
use drip_core::protocol::UploadState;

impl Store {
    pub(super) fn abort_conflicted(
        &self,
        session: &mut UploadSession,
        now: i64,
    ) -> StoreResult<()> {
        self.uploads.remove_stage(&session.stage_name)?;
        session.state = UploadState::Aborted;
        session.committing = false;
        session.has_data = false;
        session.last_progress = now;
        Ok(())
    }
}
