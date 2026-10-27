use std::{
    future::pending,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    task::JoinHandle,
    time::{MissedTickBehavior, timeout},
};

use super::{
    super::{EventKind, EventSink, SyncSnapshot, selection},
    Reachability,
};
use crate::{credentials::Pairing, transport::HttpTransport};

/// The probe never changes local selections and runs while paused or hidden.
pub(in crate::engine) struct Monitor {
    probe: JoinHandle<()>,
    transitions: JoinHandle<()>,
}

impl Monitor {
    pub(in crate::engine) fn start(
        transport: HttpTransport,
        pairing: Arc<Pairing>,
        reachability: Reachability,
        events: EventSink,
    ) -> Self {
        let probe_health = reachability.clone();
        let probe = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                match timeout(
                    Duration::from_secs(10),
                    selection::get(&transport, pairing.credential()),
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        probe_health.observe_error(&error);
                        // Malformed probe responses cannot establish authenticated reachability.
                        probe_health.failed();
                    }
                    Err(_) => probe_health.failed(),
                }
            }
        });
        let transitions = tokio::spawn(async move {
            let mut status = events.subscribe_status();
            let mut connected = false;
            loop {
                let snapshot = status.borrow_and_update().clone();
                let current = snapshot.authenticated_at(Instant::now());
                if current != connected {
                    connected = current;
                    let (kind, message) = if current {
                        (
                            EventKind::Connected,
                            "authenticated server connection confirmed",
                        )
                    } else {
                        (
                            EventKind::Disconnected,
                            "authenticated server connection unavailable; check server, certificate and pairing",
                        )
                    };
                    let _ = events.emit(kind, None, None, "connection", message).await;
                }
                tokio::select! {
                    changed = status.changed() => { if changed.is_err() { break; } }
                    _ = expiry(&snapshot) => reachability.failed(),
                }
            }
        });
        Self { probe, transitions }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.probe.abort();
        self.transitions.abort();
    }
}

async fn expiry(snapshot: &SyncSnapshot) {
    if !snapshot.authentication_failed
        && let Some(last) = snapshot.last_authenticated
    {
        tokio::time::sleep_until((last + Duration::from_secs(45) + Duration::from_nanos(1)).into())
            .await;
    } else {
        pending::<()>().await;
    }
}
