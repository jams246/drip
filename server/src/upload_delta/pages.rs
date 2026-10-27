use drip_core::protocol::{UploadPlanRequest, UploadPlanResponse};

use crate::{Store, StoreError, StoreResult, upload_store::authenticated_device};

use super::{accept, lock};

impl Store {
    /// Accepts one bounded descriptor page for an active upload session.
    pub fn plan_upload(
        &self,
        credential: &str,
        upload_id: &str,
        request: UploadPlanRequest,
    ) -> StoreResult<UploadPlanResponse> {
        let now = self.now()?;
        self.reap_uploads(now)?;
        let device = authenticated_device(self, credential)?;
        let session = self.uploads.get(upload_id, &device.pc_id, now)?;
        {
            let mut current = lock(&session);
            if current.state != drip_core::protocol::UploadState::Active
                || current.receiving
                || current.committing
                || current.planning
                || current.has_data
            {
                return Err(StoreError::Conflict);
            }
            current.planning = true;
        }
        let result = accept::plan_page(self, credential, upload_id, &session, request);
        let mut current = lock(&session);
        current.planning = false;
        if result.is_ok() && current.state == drip_core::protocol::UploadState::Active {
            current.delta_mode = true;
            current.last_progress = now;
        }
        if current.state != drip_core::protocol::UploadState::Active {
            return Err(StoreError::Conflict);
        }
        result
    }
}
