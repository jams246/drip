use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle, ThreadId},
    time::Duration,
};

use crate::upload_test_support::{CheckpointHook, CheckpointHookGuard};

pub(crate) struct CheckpointGate {
    _hook: CheckpointHookGuard,
    entered: Receiver<()>,
    release: Option<Sender<()>>,
    target_thread: Arc<Mutex<Option<ThreadId>>>,
}

impl CheckpointGate {
    pub(crate) fn new(checkpoint: &'static str) -> Self {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let target_thread = Arc::new(Mutex::new(None));
        let captured_thread = Arc::clone(&target_thread);
        let hook: CheckpointHook = Arc::new(move |name| {
            let target = *captured_thread.lock().unwrap();
            if name == checkpoint && target == Some(thread::current().id()) {
                entered_tx.send(()).unwrap();
                let _ = release_rx.lock().unwrap().recv();
            }
        });
        Self {
            _hook: CheckpointHookGuard::new(hook),
            entered,
            release: Some(release),
            target_thread,
        }
    }

    pub(crate) fn spawn<T: Send + 'static>(
        &self,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> JoinHandle<T> {
        let target = Arc::clone(&self.target_thread);
        thread::spawn(move || {
            *target.lock().unwrap() = Some(thread::current().id());
            task()
        })
    }

    pub(crate) fn wait_until_entered(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(10))
            .expect("commit reached publication checkpoint");
    }

    pub(crate) fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

impl Drop for CheckpointGate {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::CheckpointGate;

    #[test]
    fn unrelated_publications_cannot_satisfy_checkpoint_gate() {
        let mut gate = CheckpointGate::new("before_rename");
        crate::upload_test_support::checkpoint("before_rename");
        assert!(gate.entered.try_recv().is_err());
        let commit = gate.spawn(|| crate::upload_test_support::checkpoint("before_rename"));
        gate.wait_until_entered();
        gate.release();
        commit.join().unwrap();
    }
}
