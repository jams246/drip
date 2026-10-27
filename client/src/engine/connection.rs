mod monitor;

use std::time::Instant;

use tokio::sync::watch;

use super::{ConnectionState, EngineError, SyncSnapshot};

pub(super) use monitor::Monitor;

/// Authenticated exchange observations shared by the worker and its idle probe.
#[derive(Clone)]
pub(crate) struct Reachability {
    status: watch::Sender<SyncSnapshot>,
}

impl Reachability {
    pub(super) fn new(status: watch::Sender<SyncSnapshot>) -> Self {
        Self { status }
    }

    pub(crate) fn succeeded(&self) {
        self.succeeded_at(Instant::now());
    }

    fn succeeded_at(&self, now: Instant) {
        self.status.send_modify(|status| {
            status.last_authenticated = Some(now);
            status.authentication_failed = false;
            if status.connection == ConnectionState::Disconnected {
                status.connection = ConnectionState::Connected;
            }
        });
    }

    pub(crate) fn failed(&self) {
        self.status.send_modify(|status| {
            if !status.authentication_failed {
                status.failure_epoch = status.failure_epoch.saturating_add(1);
            }
            status.authentication_failed = true;
            status.connection = match status.connection {
                ConnectionState::Paused | ConnectionState::Pausing => ConnectionState::Pausing,
                _ => ConnectionState::Disconnected,
            };
        });
    }

    pub(super) fn failure_epoch(&self) -> u64 {
        self.status.borrow().failure_epoch
    }

    pub(super) fn observe_error(&self, error: &EngineError) {
        if failed_exchange(error) {
            self.failed();
        }
    }
}

fn failed_exchange(error: &EngineError) -> bool {
    match error {
        EngineError::Transport(_) => true,
        EngineError::Api { status, .. } => *status == reqwest::StatusCode::UNAUTHORIZED,
        EngineError::AbortFailed { transfer, abort } => {
            failed_exchange(transfer) || failed_exchange(abort)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
