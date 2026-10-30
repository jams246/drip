use std::{io, sync::Arc, time::Duration};

use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    server::graceful::GracefulShutdown,
    service::TowerToHyperService,
};
use tokio::{net::TcpListener, sync::Semaphore};

const MAX_CONNECTIONS: usize = 256;
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Accepts HTTP/1 connections until `shutdown` resolves, then waits for the open ones to finish.
pub(super) async fn run(
    listener: TcpListener,
    router: axum::Router,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let service = TowerToHyperService::new(router);
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    let graceful = GracefulShutdown::new();
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    tokio::pin!(shutdown);
    loop {
        let permit = tokio::select! {
            () = &mut shutdown => break,
            permit = Arc::clone(&connections).acquire_owned() => {
                permit.map_err(|_| io::Error::other("connection limit closed"))?
            }
        };
        let stream = tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                // Transient accept failures (for example EMFILE) must not stop the daemon.
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        let connection = builder.serve_connection(TokioIo::new(stream), service.clone());
        let connection = graceful.watch(connection);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = connection.await;
        });
    }
    graceful.shutdown().await;
    Ok(())
}
