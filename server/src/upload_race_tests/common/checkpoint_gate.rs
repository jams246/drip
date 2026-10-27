use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

use crate::upload_test_support::{CheckpointHook, CheckpointHookGuard};

pub(crate) struct CheckpointGate {
    _hook: CheckpointHookGuard,
    entered: Receiver<()>,
    release: Option<Sender<()>>,
}

impl CheckpointGate {
    pub(crate) fn new(checkpoint: &'static str) -> Self {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let hook: CheckpointHook = Arc::new(move |name| {
            if name == checkpoint {
                entered_tx.send(()).unwrap();
                let _ = release_rx.lock().unwrap().recv();
            }
        });
        Self {
            _hook: CheckpointHookGuard::new(hook),
            entered,
            release: Some(release),
        }
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
