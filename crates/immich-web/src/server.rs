use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::{Extension, Router};
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;

use crate::WebLimits;

pub async fn serve<Shutdown>(
    listener: TcpListener,
    router: Router,
    limits: WebLimits,
    shutdown: Shutdown,
) -> io::Result<()>
where
    Shutdown: Future<Output = ()>,
{
    let permits = Arc::new(Semaphore::new(limits.accepted_connections));
    let (stop_sender, stop_receiver) = watch::channel(false);
    let mut connections = JoinSet::new();
    let mut failure = None;
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        if let Ok(permit) = Arc::clone(&permits).try_acquire_owned() {
                            let connection_router = router.clone().layer(Extension(ConnectInfo(peer)));
                            let connection_stop = stop_receiver.clone();
                            connections.spawn(serve_connection(
                                stream,
                                connection_router,
                                limits,
                                connection_stop,
                                permit,
                            ));
                        }
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                if joined.is_some_and(|result| result.is_err()) {
                    failure = Some(io::Error::other("web connection task failed"));
                    break;
                }
            }
        }
    }
    let _shutdown_notice = stop_sender.send(true);
    while let Some(result) = connections.join_next().await {
        if result.is_err() && failure.is_none() {
            failure = Some(io::Error::other("web connection task failed"));
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn serve_connection(
    stream: TcpStream,
    router: Router,
    limits: WebLimits,
    mut shutdown: watch::Receiver<bool>,
    _permit: tokio::sync::OwnedSemaphorePermit,
) {
    let service = TowerToHyperService::new(router);
    let io = TokioIo::new(stream);
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(limits.header_read_seconds))
        .max_buf_size(limits.request_header_bytes.max(8 * 1_024));
    let connection = builder.serve_connection(io, service);
    tokio::pin!(connection);
    tokio::select! {
        _result = &mut connection => {}
        changed = shutdown.changed() => {
            if changed.is_ok() {
                connection.as_mut().graceful_shutdown();
                let _result = connection.await;
            }
        }
    }
}
