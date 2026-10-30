use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

#[tokio::test]
async fn cancelled_server_keeps_storage_owned_until_its_worker_finishes() {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.verification/security-audit/http-daemon");
    let root =
        crate::upload_test_support::create_test_dir(&parent, "cancellation", &AtomicU64::new(0));
    let gated = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicU64::new(0));
    let (started, entered) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let started = Mutex::new(Some(started));
    let wait = Mutex::new(wait);
    let clock_gate = Arc::clone(&gated);
    let clock_calls = Arc::clone(&calls);
    let store = crate::Store::open_with_clock(root.join("state"), root.join("mirror"), move || {
        clock_calls.fetch_add(1, Ordering::SeqCst);
        if clock_gate.swap(false, Ordering::SeqCst) {
            started.lock().unwrap().take().unwrap().send(()).unwrap();
            let _ = wait.lock().unwrap().recv();
        }
        Ok(100)
    })
    .unwrap();
    let token = store.create_enrollment_token().unwrap();
    let before_start = calls.load(Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let serving = tokio::spawn(super::serve(listener, store.clone()));
    let mut health = TcpStream::connect(address).await.unwrap();
    health
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), health.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls.load(Ordering::SeqCst) == before_start {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    gated.store(true, Ordering::SeqCst);
    let body = format!("{{\"token\":\"{token}\",\"label\":\"Worker cancellation\"}}");
    let request = format!(
        "POST /v1/enroll HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(request.as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    serving.abort();
    let _ = serving.await;
    drop(client);
    let fenced = crate::Store::open_for_daemon(root.join("state"), root.join("mirror")).is_err();
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(replacement) =
                crate::Store::open_for_daemon(root.join("state"), root.join("mirror"))
            {
                drop(replacement);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(fenced);
    assert_eq!(store.list_devices().unwrap().len(), 1);
    drop(health);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
