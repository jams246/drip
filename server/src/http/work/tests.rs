use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use tokio::sync::{Semaphore, oneshot};

use super::ControlWork;

#[tokio::test]
async fn disconnected_control_keeps_admission_until_blocking_work_finishes() {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.verification/security-audit/http-workers");
    let root = crate::upload_test_support::create_test_dir(&parent, "worker", &AtomicU64::new(0));
    let store = crate::Store::open(root.join("state"), root.join("mirror")).unwrap();
    let controls = Arc::new(Semaphore::new(32));
    let workers = Arc::new(Semaphore::new(8));
    let work = ControlWork {
        store,
        workers: Arc::clone(&workers),
        admission: Arc::new(Arc::clone(&controls).try_acquire_owned().unwrap()),
        lease: None,
    };
    let (started, entered) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let request = tokio::spawn(async move {
        work.run(move |_| {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(())
        })
        .await
    });
    entered.await.unwrap();
    request.abort();
    let _ = request.await;
    let retained = (controls.available_permits(), workers.available_permits());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while controls.available_permits() != 32 || workers.available_permits() != 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(retained, (31, 7));
    std::fs::remove_dir_all(root).unwrap();
}
