use std::time::{Duration, Instant};

use super::*;

fn health() -> (Reachability, watch::Receiver<SyncSnapshot>) {
    let (sender, receiver) = watch::channel(SyncSnapshot::default());
    (Reachability::new(sender), receiver)
}

#[test]
fn footer_requires_a_current_authenticated_exchange() {
    let (health, status) = health();
    let now = Instant::now();
    assert_eq!(status.borrow().footer_label(now), "Disconnected");
    health.succeeded_at(now);
    assert_eq!(status.borrow().footer_label(now), "Connected");
    assert_eq!(
        status.borrow().footer_label(now + Duration::from_secs(45)),
        "Connected"
    );
    assert_eq!(
        status.borrow().footer_label(now + Duration::from_secs(46)),
        "Disconnected"
    );
}

#[test]
fn source_errors_do_not_change_server_reachability() {
    let (health, status) = health();
    let now = Instant::now();
    health.succeeded_at(now);
    let event = super::super::LifecycleEvent {
        timestamp: drip_core::Timestamp::new(1, 0).unwrap(),
        kind: super::super::EventKind::UnavailableRoot,
        selection_id: Some("offline-drive".into()),
        source_path: Some(drip_core::SourcePath::new('C', vec!["missing".into()]).unwrap()),
        operation_id: "source-read".into(),
        message: "Selected drive is unavailable".into(),
        progress: None,
    };
    super::super::status::update(&health.status, &event);
    assert_eq!(
        status.borrow().roots[0].state,
        super::super::SelectionState::UnavailableRoot
    );
    assert_eq!(status.borrow().footer_label(now), "Connected");
}

#[test]
fn pending_pause_and_reconnection_require_a_new_barrier_acknowledgement() {
    let (health, status) = health();
    let now = Instant::now();
    health.succeeded_at(now);
    health
        .status
        .send_modify(|snapshot| snapshot.connection = ConnectionState::Pausing);
    assert_eq!(status.borrow().footer_label(now), "Connected");
    health
        .status
        .send_modify(|snapshot| snapshot.connection = ConnectionState::Paused);
    assert_eq!(status.borrow().footer_label(now), "Paused");
    let epoch = health.failure_epoch();
    health.failed();
    assert_eq!(status.borrow().footer_label(now), "Disconnected");
    assert_ne!(health.failure_epoch(), epoch);
    health.succeeded_at(now);
    assert_eq!(status.borrow().connection, ConnectionState::Pausing);
    assert_eq!(status.borrow().footer_label(now), "Connected");
    health
        .status
        .send_modify(|snapshot| snapshot.connection = ConnectionState::Paused);
    assert_eq!(status.borrow().footer_label(now), "Paused");
}

#[test]
fn repeated_failed_probes_retain_one_failure_epoch_until_reconnect() {
    let (health, _) = health();
    health.failed();
    let epoch = health.failure_epoch();
    health.failed();
    assert_eq!(health.failure_epoch(), epoch);
    health.succeeded();
    health.failed();
    assert_eq!(health.failure_epoch(), epoch + 1);
}
