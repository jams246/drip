use std::{io, sync::Arc, time::Duration};

use tokio::{net::TcpListener, sync::watch};

use crate::Store;

/// Serves the router and stops on process termination signals.
pub async fn serve(listener: TcpListener, store: Store) -> io::Result<()> {
    let startup = store.clone();
    let lease = tokio::task::spawn_blocking(move || {
        let lease = startup.take_daemon_lease()?;
        startup.prepare_daemon()?;
        Ok::<_, crate::StoreError>(Arc::new(lease))
    })
    .await
    .map_err(io::Error::other)?
    .map_err(io::Error::other)?;
    let (stop, mut stopped) = watch::channel(false);
    let expiry_store = store.clone();
    let expiry_lease = Arc::clone(&lease);
    let reaper = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                _ = interval.tick() => {}
            }
            let store = expiry_store.clone();
            let lease = Arc::clone(&expiry_lease);
            let _ = tokio::task::spawn_blocking(move || {
                let _lease = lease;
                if let Ok(now) = store.now() {
                    let _ = store.reap_uploads(now);
                }
            })
            .await;
        }
    });
    let mut state = super::HttpState::new(store);
    state.lease = Some(lease);
    let result = accept::run(
        listener,
        super::router_with_state(state.clone()),
        shutdown_signal(),
    )
    .await;
    let _ = stop.send(true);
    let _ = reaper.await;
    state.drain().await;
    result
}

mod accept;

#[cfg(test)]
mod tests;

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let terminate = signal(SignalKind::terminate());
        if let Ok(mut terminate) = terminate {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
