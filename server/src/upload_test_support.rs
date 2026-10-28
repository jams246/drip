use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) fn create_test_dir(parent: &Path, prefix: &str, next: &AtomicU64) -> PathBuf {
    std::fs::create_dir_all(parent).unwrap();
    loop {
        let suffix = next.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!("{}-{prefix}-{suffix}", std::process::id()));
        match std::fs::create_dir(&root) {
            Ok(()) => return root,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create test directory {}: {error}", root.display()),
        }
    }
}

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
    fn stale_fixture_is_preserved_when_a_process_id_is_reused() {
        let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.verification/gui-refresh/fixture-collision");
        let root = super::create_test_dir(&parent, "collision", &super::AtomicU64::new(0));
        std::fs::write(root.join("marker"), b"preserve").unwrap();
        let next = super::create_test_dir(&parent, "collision", &super::AtomicU64::new(0));
        assert_ne!(root, next);
        assert_eq!(std::fs::read(root.join("marker")).unwrap(), b"preserve");
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(next).unwrap();
    }

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
