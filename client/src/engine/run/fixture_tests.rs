mod deletions;
mod support;

use std::{fs, os::windows::fs::OpenOptionsExt, time::Duration};

use crate::engine::{EventKind, LifecycleEvent};
use support::Fixture;

fn processed(events: &[LifecycleEvent], id: &str) -> bool {
    events.iter().any(|event| {
        event.selection_id.as_deref() == Some(id)
            && matches!(
                event.kind,
                EventKind::Scanning
                    | EventKind::Hashing
                    | EventKind::Chunking
                    | EventKind::Uploading
                    | EventKind::UploadStarted
            )
    })
}

fn content(events: &[LifecycleEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event.kind,
            EventKind::Hashing
                | EventKind::Chunking
                | EventKind::Uploading
                | EventKind::UploadStarted
        )
    })
}

#[tokio::test]
#[ignore = "requires the owned watch-isolation HTTPS Docker fixture and an isolated test executable"]
async fn real_watch_events_isolate_roots_and_periodic_checks_reuse_verified_contents() {
    let mut fixture = Fixture::new().await;
    let folder = fixture.sources.join("folder-a");
    fs::create_dir_all(&folder).unwrap();
    for index in 0..20 {
        fs::write(
            folder.join(format!("file-{index:02}.txt")),
            format!("synthetic {index}"),
        )
        .unwrap();
    }
    let a = fixture.engine.add(&folder).unwrap().selection.id;
    assert!(content(&fixture.pass(true, true).await));

    let text = fixture.sources.join("test.txt");
    fs::write(&text, b"initial text").unwrap();
    let b = fixture.engine.add(&text).unwrap().selection.id;
    fixture.signal.refresh();
    let events = fixture.pass(false, false).await;
    assert!(
        !processed(&events, &a),
        "adding a selection processed unrelated folder"
    );
    assert!(processed(&events, &b));

    let before = fixture.signal.epoch(&b);
    fs::write(&text, b"edited text").unwrap();
    fixture.changed(&b, before).await;
    let events = fixture.pass(false, false).await;
    assert!(
        !processed(&events, &a),
        "editing text processed unrelated folder"
    );
    assert!(content(&events));

    let a_before = fixture.signal.epoch(&a);
    let b_before = fixture.signal.epoch(&b);
    fs::write(fixture.sources.join("unwatched.txt"), b"unwatched").unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!fixture.signal.changed_since(&a, a_before));
    assert!(!fixture.signal.changed_since(&b, b_before));

    let events = fixture.pass(true, false).await;
    assert!(events.iter().any(|event| event.kind == EventKind::Scanning));
    assert!(!content(&events), "warm periodic check read file contents");

    let before = fixture.signal.epoch(&a);
    let changed = folder.join("file-00.txt");
    fs::write(&changed, b"one changed file").unwrap();
    fixture.changed(&a, before).await;
    let events = fixture.pass(false, false).await;
    assert!(!processed(&events, &b));
    let hashed: Vec<_> = events
        .iter()
        .filter(|event| event.kind == EventKind::Hashing)
        .collect();
    assert_eq!(hashed.len(), 1);
    assert_eq!(
        hashed[0].source_path.as_ref().unwrap(),
        &drip_core::SourcePath::try_from_path(&changed).unwrap()
    );

    fixture.change_server(&text);
    let events = fixture.pass(true, false).await;
    assert!(events.iter().any(|event| event.kind == EventKind::Chunking));
    assert!(!events.iter().any(|event| event.kind == EventKind::Hashing));
    assert!(
        events
            .iter()
            .filter_map(|event| event.progress.as_ref())
            .all(|progress| progress.source_hashed_bytes == 0)
    );
    assert!(!content(&fixture.pass(true, false).await));

    fixture.signal.mark(&b, true);
    let events = fixture.pass(false, false).await;
    assert!(!processed(&events, &a));
    assert!(events.iter().any(|event| event.kind == EventKind::Hashing));
    assert!(content(&fixture.pass(true, true).await));

    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&text)
        .unwrap();
    fixture.signal.mark(&b, false);
    let events = fixture.attempt(false, false).await.1;
    assert!(!processed(&events, &a));
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::UnavailableRoot)
    );
    let retry = fixture.attempt(false, false).await.1;
    assert!(!processed(&retry, &a), "retry restarted successful folder");
    drop(locked);
    fixture.pass(false, false).await;
}
