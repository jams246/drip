use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::Notify;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChangeEpoch(u64, u64, u64);

#[derive(Default)]
pub(crate) struct DirtyBatch {
    pub selections: HashSet<String>,
    pub uncertain: HashSet<String>,
    pub full: bool,
    pub all: bool,
}

#[derive(Default)]
struct State {
    batch: DirtyBatch,
    epochs: HashMap<String, u64>,
    settings: bool,
    settings_epoch: u64,
    global_epoch: u64,
}

#[derive(Clone, Default)]
pub(crate) struct DirtySignal {
    state: Arc<Mutex<State>>,
    wake: Arc<Notify>,
}

impl DirtySignal {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| {
            let mut state = poisoned.into_inner();
            state.batch.full = true;
            state.global_epoch = state.global_epoch.wrapping_add(1);
            state
        })
    }

    pub fn mark(&self, selection: &str, uncertain: bool) {
        let mut state = self.state();
        state.batch.selections.insert(selection.to_owned());
        if uncertain {
            state.batch.uncertain.insert(selection.to_owned());
        }
        let epoch = state.epochs.entry(selection.to_owned()).or_default();
        *epoch = epoch.wrapping_add(1);
        drop(state);
        self.wake.notify_one();
    }

    pub fn refresh(&self) {
        let mut state = self.state();
        state.settings = true;
        state.settings_epoch = state.settings_epoch.wrapping_add(1);
        drop(state);
        self.wake.notify_one();
    }

    pub fn overflow(&self) {
        let mut state = self.state();
        state.batch.full = true;
        state.global_epoch = state.global_epoch.wrapping_add(1);
        drop(state);
        self.wake.notify_one();
    }

    pub fn retry(&self, selection: &str) {
        self.state().batch.selections.insert(selection.to_owned());
    }

    pub fn sweep(&self) {
        self.state().batch.all = true;
    }

    pub fn take(&self) -> DirtyBatch {
        let mut state = self.state();
        state.settings = false;
        std::mem::take(&mut state.batch)
    }

    pub fn pending(&self) -> bool {
        let state = self.state();
        state.settings || state.batch.full || state.batch.all || !state.batch.selections.is_empty()
    }

    pub fn full(&self) -> bool {
        self.state().batch.full
    }

    pub fn rebuild(&self) -> bool {
        let state = self.state();
        state.batch.full || !state.batch.uncertain.is_empty()
    }

    pub fn epoch(&self, selection: &str) -> ChangeEpoch {
        let state = self.state();
        ChangeEpoch(
            *state.epochs.get(selection).unwrap_or(&0),
            state.settings_epoch,
            state.global_epoch,
        )
    }

    pub fn changed_since(&self, selection: &str, epoch: ChangeEpoch) -> bool {
        self.epoch(selection) != epoch
    }

    pub async fn changed(&self) {
        self.wake.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_roots_do_not_invalidate_completion_but_settings_and_overflow_do() {
        let signal = DirtySignal::default();
        let epoch = signal.epoch("a");
        signal.mark("b", false);
        assert!(!signal.changed_since("a", epoch));
        signal.refresh();
        assert!(signal.changed_since("a", epoch));
        let epoch = signal.epoch("a");
        signal.overflow();
        assert!(signal.changed_since("a", epoch));
    }

    #[test]
    fn sweep_and_full_verification_remain_pending_until_a_batch_starts() {
        let signal = DirtySignal::default();
        signal.sweep();
        assert!(signal.pending());
        assert!(!signal.full());
        let batch = signal.take();
        assert!(batch.all && !batch.full);
        signal.overflow();
        signal.sweep();
        assert!(signal.full());
        let batch = signal.take();
        assert!(batch.all && batch.full);
        assert!(!signal.pending());
    }

    #[test]
    fn taking_a_batch_preserves_epochs_and_subsequent_events() {
        let signal = DirtySignal::default();
        signal.mark("a", true);
        let epoch = signal.epoch("a");
        let batch = signal.take();
        assert!(batch.selections.contains("a") && batch.uncertain.contains("a"));
        assert!(!signal.pending());
        assert!(!signal.changed_since("a", epoch));
        signal.mark("a", false);
        assert!(signal.changed_since("a", epoch));
        assert!(signal.take().selections.contains("a"));
    }

    #[tokio::test]
    async fn internal_retries_do_not_wake_or_invalidate_other_work() {
        let signal = DirtySignal::default();
        let epoch = signal.epoch("a");
        signal.retry("a");
        assert!(signal.pending());
        assert!(!signal.changed_since("a", epoch));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), signal.changed())
                .await
                .is_err()
        );
    }
}
