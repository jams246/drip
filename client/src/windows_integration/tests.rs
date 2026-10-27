use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use tokio::{
    sync::{mpsc, watch},
    time::timeout,
};

use super::{InstanceAcquire, InstanceError, InstanceMode, acquire};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn mutex_treats_canonical_directory_alias_as_same_instance() {
    let directory = TestDirectory::new();
    let InstanceAcquire::Acquired(_guard) = acquire(&directory.path, InstanceMode::Diagnostic)
        .await
        .unwrap()
    else {
        panic!("first process should acquire the mutex");
    };

    let result = acquire(&directory.path.join("."), InstanceMode::Diagnostic)
        .await
        .unwrap();

    assert!(matches!(result, InstanceAcquire::AlreadyRunning));
}

#[tokio::test]
async fn second_gui_forwards_open_through_bounded_listener() {
    let directory = TestDirectory::new();
    let InstanceAcquire::Acquired(mut guard) =
        acquire(&directory.path, InstanceMode::Gui).await.unwrap()
    else {
        panic!("first GUI should acquire the mutex");
    };
    let listener = guard.take_open_listener().unwrap();
    let (sender, mut receiver) = mpsc::channel(1);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let listener_task = tokio::spawn(async move {
        listener
            .run(move || sender.try_send(()).is_ok(), shutdown_rx)
            .await
    });

    let result = acquire(&directory.path, InstanceMode::Gui).await.unwrap();

    assert!(matches!(result, InstanceAcquire::AlreadyRunning));
    assert!(
        timeout(Duration::from_secs(1), receiver.recv())
            .await
            .unwrap()
            .is_some()
    );
    shutdown_tx.send(true).unwrap();
    listener_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn gui_reports_diagnostic_owner_without_starting_second_instance() {
    let directory = TestDirectory::new();
    let InstanceAcquire::Acquired(_guard) = acquire(&directory.path, InstanceMode::Diagnostic)
        .await
        .unwrap()
    else {
        panic!("diagnostic process should acquire the mutex");
    };

    let result = acquire(&directory.path, InstanceMode::Gui).await;

    assert!(matches!(
        result,
        Err(InstanceError::OpenListenerUnavailable)
    ));
}

struct TestDirectory {
    root: PathBuf,
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let root = workspace.join(".verification/phase-04/instance-tests");
        fs::create_dir_all(&root).unwrap();
        loop {
            let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("test-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { root, path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create verification directory: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.path.parent() == Some(self.root.as_path()) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
