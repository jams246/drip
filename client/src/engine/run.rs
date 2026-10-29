mod locking;
mod passes;
mod setup;

#[cfg(test)]
mod fixture_tests;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Instant};

use tokio::sync::watch;

use super::{
    Engine, EngineError, EventKind, EventSink,
    pass::auth_failure,
    saved,
    schedule::{Schedule, backoff},
    watcher::DirtySignal,
};
use crate::{source::random_id, transport::HttpTransport};
use locking::{pristine_stop, worker_lock};

/// Desired worker controls. Paused is acknowledged only by a lifecycle barrier event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RunControl {
    pub paused: bool,
    pub stop: bool,
    /// Increment to request full reconciliation without waiting for its timer.
    pub reconcile: u64,
    /// Increment to synchronize selection changes without a full content rescan.
    pub refresh_selections: u64,
}

impl Engine {
    /// Runs one continuous worker. The caller consumes lifecycle events independently.
    /// Closing the control channel requests a fenced stop.
    pub async fn run(
        &self,
        mut control: watch::Receiver<RunControl>,
        events: EventSink,
    ) -> Result<(), EngineError> {
        let _worker_lock = worker_lock(self.settings.directory())?;
        let events = events.with_log(self.settings.directory());
        let pairing = Arc::new(self.credentials.load()?);
        let reachability = events.reachability();
        let transport =
            HttpTransport::new(pairing.server_url())?.with_reachability(reachability.clone());
        let _connection = super::connection::Monitor::start(
            transport.clone(),
            pairing.clone(),
            reachability.clone(),
            events.clone(),
        );
        let mut connection = events.subscribe_status();
        let signal = DirtySignal::default();
        signal.overflow();
        let cache = super::cache::FileCache::default();
        let mut state = setup::PassState::default();
        let mut schedule = Schedule::new(Instant::now());
        let mut barrier = true;
        let mut paused = false;
        let mut acknowledged_failure_epoch = 0;
        let mut attempts = 0u32;
        let mut closed = false;
        let mut reconciled_request = 0;
        let mut refreshed_request = 0;
        loop {
            let desired = *control.borrow_and_update();
            if desired.reconcile != reconciled_request {
                reconciled_request = desired.reconcile;
                signal.overflow();
                schedule.force(Instant::now());
            }
            if desired.refresh_selections != refreshed_request {
                refreshed_request = desired.refresh_selections;
                signal.refresh();
                schedule.hint(Instant::now());
            }
            if pristine_stop(&self.settings, &control, closed) {
                return Ok(());
            }
            if desired.paused || desired.stop || closed {
                if paused && reachability.failure_epoch() != acknowledged_failure_epoch {
                    paused = false;
                    barrier = true;
                }
                if !paused {
                    let pause_operation = random_id()?;
                    events
                        .emit(
                            EventKind::Pausing,
                            None,
                            None,
                            &pause_operation,
                            "waiting for selection mutation barrier",
                        )
                        .await?;
                    loop {
                        if pristine_stop(&self.settings, &control, closed) {
                            return Ok(());
                        }
                        let epoch = reachability.failure_epoch();
                        match saved::synchronize(&self.settings, &transport, &pairing, true).await {
                            Ok(_) if reachability.failure_epoch() == epoch => {
                                acknowledged_failure_epoch = epoch;
                                break;
                            }
                            Ok(_) => continue,
                            Err(error) if auth_failure(&error) => return Err(error),
                            Err(error) => {
                                reachability.observe_error(&error);
                                events
                                    .emit(
                                        EventKind::Retry,
                                        None,
                                        None,
                                        &pause_operation,
                                        format!("pause barrier pending: {error}"),
                                    )
                                    .await?;
                                if closed {
                                    tokio::time::sleep(backoff(attempts)?).await;
                                } else {
                                    tokio::select! {
                                        changed = control.changed() => { closed = changed.is_err(); }
                                        _ = tokio::time::sleep(backoff(attempts)?) => {}
                                    }
                                }
                                attempts = attempts.saturating_add(1);
                            }
                        }
                    }
                    state.watcher = None;
                    events
                        .emit(
                            EventKind::Paused,
                            None,
                            None,
                            &pause_operation,
                            "selection barrier acknowledged",
                        )
                        .await?;
                    paused = true;
                }
                let current = *control.borrow_and_update();
                if current.stop || closed {
                    return Ok(());
                }
                if !current.paused {
                    continue;
                }
                tokio::select! {
                    changed = control.changed() => { closed = changed.is_err(); }
                    _ = connection.changed() => {}
                }
                continue;
            }
            if paused {
                paused = false;
                signal.overflow();
                barrier = true;
                schedule.force(Instant::now());
                events
                    .emit(
                        EventKind::Resumed,
                        None,
                        None,
                        "resume",
                        "full reconciliation requested",
                    )
                    .await?;
            }
            let due = tokio::time::Instant::from_std(schedule.due());
            tokio::select! {
                changed = control.changed() => {
                    closed = changed.is_err();
                }
                _ = signal.changed() => {
                    if signal.full() { schedule.force(Instant::now()); }
                    else if signal.pending() { schedule.hint(Instant::now()); }
                }
                _ = tokio::time::sleep_until(due) => {
                    if schedule.begin(Instant::now()) { signal.sweep(); }
                    let context = super::pass::PassContext { cache: &cache, store: &self.settings,
                        transport: &transport, pairing: &pairing, events: &events, signal: &signal, control: &control };
                    let result = self.continuous_pass(&context, &mut state, barrier).await;
                    if let Err(error) = &result { reachability.observe_error(error); }
                    match result {
                        Ok(true) => {
                            barrier = false;
                            attempts = 0;
                            if signal.pending() { schedule.hint(Instant::now()); }
                        }
                        Err(EngineError::Cancelled) => continue,
                        Err(EngineError::SelectionChanged) => {
                            barrier = true;
                            schedule.retry(Instant::now());
                        }
                        Err(error) if auth_failure(&error) => {
                            events.emit(EventKind::Error, None, None, "authentication", "registration rejected; repair pairing before retrying").await?;
                            return Err(error);
                        }
                        result => {
                            let operation = random_id()?;
                            if result.is_ok() { barrier = false; }
                            if let Err(error) = result {
                                barrier = true;
                                events.emit(EventKind::Disconnected, None, None, &operation, error.to_string()).await?;
                            }
                            events.emit(EventKind::Retry, None, None, &operation, "reconciliation incomplete; retry scheduled").await?;
                            tokio::select! {
                                changed = control.changed() => { closed = changed.is_err(); }
                                _ = signal.changed() => { tokio::time::sleep(super::schedule::DEBOUNCE).await; }
                                _ = tokio::time::sleep(backoff(attempts)?) => {}
                            }
                            attempts = attempts.saturating_add(1);
                            schedule.retry(Instant::now());
                        }
                    }
                }
            }
        }
    }
}
