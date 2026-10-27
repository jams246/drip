use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use cap_std::fs::Dir;
use drip_core::protocol::{EntryKind, EntryPrecondition, UploadState};
use drip_core::{Blake3Digest, SourcePath, Timestamp};

use crate::{StoreError, StoreResult};

const MAX_ACTIVE: usize = 8;
const SESSION_TTL: i64 = 30 * 60;

pub(crate) struct UploadSessions {
    mirror: Arc<Dir>,
    sessions: Mutex<HashMap<String, Arc<Mutex<UploadSession>>>>,
}

pub(crate) struct UploadSession {
    pub(crate) id: String,
    pub(crate) pc_id: String,
    pub(crate) selection_id: String,
    pub(crate) selection_root: SourcePath,
    pub(crate) generation: u64,
    pub(crate) source_path: SourcePath,
    pub(crate) expected_size: u64,
    pub(crate) digest: Blake3Digest,
    pub(crate) last_write: Timestamp,
    pub(crate) expected_entry: Option<EntryPrecondition>,
    pub(crate) base_revision: Option<u64>,
    pub(crate) base_snapshot: Option<super::entry_files::EntrySnapshot>,
    pub(crate) stage_name: String,
    pub(crate) state: UploadState,
    pub(crate) last_progress: i64,
    pub(crate) receiving: bool,
    pub(crate) committing: bool,
    pub(crate) has_data: bool,
    pub(crate) delta_mode: bool,
    pub(crate) planning: bool,
}

impl UploadSessions {
    pub(crate) fn new(mirror: Arc<Dir>) -> Self {
        Self {
            mirror,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn create_stage(&self, id: &str) -> StoreResult<String> {
        super::upload_staging::create(&self.mirror, id)
    }

    pub(crate) fn create_directory_stage(&self, id: &str) -> StoreResult<String> {
        super::upload_staging::create_directory_stage(&self.mirror, id)
    }

    pub(crate) fn open_stage_for_write(&self, name: &str) -> StoreResult<cap_std::fs::File> {
        super::upload_staging::open_write(&self.mirror, name)
    }

    pub(crate) fn open_delta_stage(&self, name: &str) -> StoreResult<cap_std::fs::File> {
        super::upload_staging::open_delta_write(&self.mirror, name)
    }

    pub(crate) fn staging_dir(&self) -> StoreResult<Dir> {
        super::upload_staging::open_dir(&self.mirror)
    }

    pub(crate) fn remove_stage(&self, name: &str) -> StoreResult<()> {
        super::upload_staging::remove(&self.mirror, name)
    }

    pub(crate) fn remove_retired_stage(&self, name: &str, kind: EntryKind) -> StoreResult<()> {
        super::upload_staging::remove_retired(&self.mirror, name, kind)
    }

    pub(crate) fn remove_directory_stage(&self, name: &str) -> StoreResult<()> {
        super::upload_staging::remove_directory_stage(&self.mirror, name)
    }

    pub(crate) fn reserve(&self, session: UploadSession) -> StoreResult<()> {
        self.reap_expired(session.last_progress);
        let mut stale = Vec::new();
        let result = {
            let mut sessions = lock(&self.sessions);
            for current in sessions.values() {
                let mut current = lock(current);
                if current.state == UploadState::Active
                    && current.pc_id == session.pc_id
                    && current.generation < session.generation
                {
                    current.state = UploadState::Aborted;
                    current.receiving = false;
                    current.committing = false;
                    current.has_data = false;
                    current.last_progress = session.last_progress;
                    stale.push(current.stage_name.clone());
                }
            }
            let active = sessions
                .values()
                .filter(|current| lock(current).state == UploadState::Active)
                .count();
            let device_active = sessions.values().any(|current| {
                let current = lock(current);
                current.pc_id == session.pc_id && current.state == UploadState::Active
            });
            if active >= MAX_ACTIVE || device_active {
                Err(StoreError::CapacityExhausted)
            } else if sessions.contains_key(&session.id) {
                Err(StoreError::Conflict)
            } else {
                sessions.insert(session.id.clone(), Arc::new(Mutex::new(session)));
                Ok(())
            }
        };
        for name in stale {
            let _ = self.remove_stage(&name);
        }
        result
    }

    pub(crate) fn get(
        &self,
        id: &str,
        pc_id: &str,
        now: i64,
    ) -> StoreResult<Arc<Mutex<UploadSession>>> {
        self.reap_expired(now);
        let session = lock(&self.sessions)
            .get(id)
            .cloned()
            .ok_or(StoreError::NotFound)?;
        if lock(&session).pc_id != pc_id {
            return Err(StoreError::NotFound);
        }
        Ok(session)
    }

    pub(crate) fn abort_obsolete_ids(&self, pc_id: &str, generation: u64, now: i64) -> Vec<String> {
        let mut stale = Vec::new();
        let mut ids = Vec::new();
        for session in lock(&self.sessions).values() {
            let mut session = lock(session);
            if session.pc_id == pc_id
                && session.state == UploadState::Active
                && session.generation < generation
            {
                session.state = UploadState::Aborted;
                session.receiving = false;
                session.committing = false;
                session.has_data = false;
                session.last_progress = now;
                stale.push(session.stage_name.clone());
                ids.push(session.id.clone());
            }
        }
        for name in stale {
            let _ = self.remove_stage(&name);
        }
        ids
    }

    pub(crate) fn remove(&self, id: &str) -> Option<Arc<Mutex<UploadSession>>> {
        lock(&self.sessions).remove(id)
    }

    pub(crate) fn clean_abandoned_staging(&self) -> StoreResult<()> {
        lock(&self.sessions).clear();
        super::upload_staging::clean(&self.mirror)
    }

    pub(crate) fn reap_expired(&self, now: i64) {
        let _ = self.reap_expired_ids(now);
    }

    pub(crate) fn reap_expired_ids(&self, now: i64) -> Vec<String> {
        let mut expired = Vec::new();
        let mut expired_ids = Vec::new();
        {
            let mut sessions = lock(&self.sessions);
            sessions.retain(|id, session| {
                let session = lock(session);
                let keep = now.saturating_sub(session.last_progress) < SESSION_TTL;
                if !keep && session.state == UploadState::Active {
                    expired.push(session.stage_name.clone());
                    expired_ids.push(id.clone());
                }
                keep
            });
        }
        for name in expired {
            let _ = self.remove_stage(&name);
        }
        expired_ids
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
