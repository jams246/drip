use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use crate::{StoreError, upload_test_support::CheckpointHookGuard};

use super::common::{Fixture, assert_old_bytes};

#[tokio::test]
async fn expired_commit_releases_session_before_reaper_cleanup() {
    let mut fixture = Fixture::new().await;
    let session = fixture
        .store
        .uploads
        .get(&fixture.upload_id, &fixture.pc_id, 100)
        .unwrap();
    session.lock().unwrap().last_progress = 100;
    let now = Arc::new(AtomicI64::new(100));
    let captured_now = Arc::clone(&now);
    fixture.store.clock = Arc::new(move || Ok(captured_now.load(Ordering::SeqCst)));
    let (entered_tx, entered) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let release = Mutex::new(release);
    let target = Arc::new(Mutex::new(None));
    let captured_target = Arc::clone(&target);
    let reaper_store = fixture.store.clone();
    let _hook = CheckpointHookGuard::new(Arc::new(move |name| {
        if *captured_target.lock().unwrap() != Some(thread::current().id()) {
            return;
        }
        if name == "before_writer_lock" {
            entered_tx.send(()).unwrap();
            release.lock().unwrap().recv().unwrap();
        } else if name == "expired_session_before_remove" {
            let store = reaper_store.clone();
            let (done_tx, done) = mpsc::channel();
            let reaper = thread::spawn(move || {
                store.uploads.reap_expired(1900);
                done_tx.send(()).unwrap();
            });
            done.recv_timeout(Duration::from_secs(2))
                .expect("expiry cleanup must acquire the session while commit waits");
            reaper.join().unwrap();
        }
    }));
    let store = fixture.store.clone();
    let credential = fixture.credential.clone();
    let upload_id = fixture.upload_id.clone();
    let commit = thread::spawn(move || {
        *target.lock().unwrap() = Some(thread::current().id());
        store.commit_upload(&credential, &upload_id)
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    now.store(1900, Ordering::SeqCst);
    release_tx.send(()).unwrap();
    assert!(matches!(commit.join().unwrap(), Err(StoreError::NotFound)));
    assert_old_bytes(&fixture);
    assert!(matches!(
        fixture
            .store
            .get_upload_state(&fixture.credential, &fixture.upload_id),
        Err(StoreError::NotFound)
    ));
}
