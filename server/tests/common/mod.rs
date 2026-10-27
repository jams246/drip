use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use drip_server::Store;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub _root: PathBuf,
    pub store: Store,
}

impl Fixture {
    pub fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-01/server-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!("{}-{suffix}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("state"), root.join("mirror")).unwrap();
        Self { _root: root, store }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self._root);
    }
}
