use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle, ThreadId},
};

use crate::upload_test_support::{CheckpointHook, CheckpointHookGuard};

pub(crate) struct PanicAtCheckpoint {
    _guard: CheckpointHookGuard,
    target_thread: Arc<Mutex<Option<ThreadId>>>,
    hits: Arc<AtomicUsize>,
    checkpoint: &'static str,
}

impl PanicAtCheckpoint {
    pub(crate) fn new(checkpoint: &'static str) -> Self {
        let target_thread = Arc::new(Mutex::new(None));
        let captured_thread = Arc::clone(&target_thread);
        let hits = Arc::new(AtomicUsize::new(0));
        let captured_hits = Arc::clone(&hits);
        let hook: CheckpointHook = Arc::new(move |name| {
            let is_target = *captured_thread.lock().unwrap() == Some(thread::current().id());
            if name == checkpoint && is_target {
                captured_hits.fetch_add(1, Ordering::SeqCst);
                panic!("simulated interruption at {checkpoint}");
            }
        });
        Self {
            _guard: CheckpointHookGuard::new(hook),
            target_thread,
            hits,
            checkpoint,
        }
    }

    pub(crate) fn interrupt(&self, mutation: impl FnOnce() + Send + 'static) {
        let target = Arc::clone(&self.target_thread);
        let mutation: JoinHandle<()> = thread::spawn(move || {
            *target.lock().unwrap() = Some(thread::current().id());
            mutation();
        });
        assert!(
            mutation.join().is_err(),
            "mutation did not panic at {}",
            self.checkpoint
        );
        assert_eq!(self.hits.load(Ordering::SeqCst), 1);
    }
}
