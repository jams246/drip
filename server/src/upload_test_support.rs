use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

pub(crate) type CheckpointHook = Arc<dyn Fn(&'static str) + Send + Sync>;

static CHECKPOINT_HOOK: OnceLock<Mutex<Option<CheckpointHook>>> = OnceLock::new();
static CHECKPOINT_TEST_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn checkpoint(name: &'static str) {
    if let Some(hook) = CHECKPOINT_HOOK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        hook(name);
    }
}

pub(crate) struct CheckpointHookGuard {
    _lock: MutexGuard<'static, ()>,
}

impl CheckpointHookGuard {
    pub(crate) fn new(hook: CheckpointHook) -> Self {
        let lock = CHECKPOINT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *CHECKPOINT_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
        Self { _lock: lock }
    }
}

impl Drop for CheckpointHookGuard {
    fn drop(&mut self) {
        *CHECKPOINT_HOOK
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{CheckpointHookGuard, checkpoint};

    #[test]
    fn publication_checkpoint_hook_can_gate_test_races() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let _guard = CheckpointHookGuard::new(Arc::new(move |name| {
            if name == "hook_self_test" {
                captured.lock().unwrap().push(name);
            }
        }));
        checkpoint("hook_self_test");
        assert_eq!(*seen.lock().unwrap(), ["hook_self_test"]);
    }
}
