#![forbid(unsafe_code)]

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::{sleep, timeout};

#[allow(dead_code)]
mod support;

use support::TestWorkspace;

#[tokio::test]
async fn development_listener_enforces_header_deadline_and_joins_on_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let console = workspace.console_on(address.port(), "http://127.0.0.1:9387")?;
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let server = tokio::spawn(console.serve_on(listener, async {
        let _shutdown_result = shutdown_receiver.await;
    }));

    let mut client = TcpStream::connect(address).await?;
    client
        .write_all(
            format!("GET / HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    client.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("content-security-policy:"));
    assert!(response.contains("Pair this browser"));

    let mut slow_client = TcpStream::connect(address).await?;
    let mut second_slow_client = TcpStream::connect(address).await?;
    slow_client.write_all(b"GET / HTTP/1.1\r\nHost:").await?;
    second_slow_client
        .write_all(b"GET / HTTP/1.1\r\nHost:")
        .await?;
    sleep(Duration::from_millis(100)).await;
    let mut excess_client = TcpStream::connect(address).await?;
    excess_client
        .write_all(format!("GET / HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes())
        .await?;
    let mut excess_response = Vec::new();
    let excess_read = timeout(
        Duration::from_secs(1),
        excess_client.read_to_end(&mut excess_response),
    )
    .await?;
    if let Err(error) = excess_read
        && error.kind() != std::io::ErrorKind::ConnectionReset
    {
        return Err(error.into());
    }
    assert!(!excess_response.windows(6).any(|bytes| bytes == b"200 OK"));

    sleep(Duration::from_millis(1_100)).await;
    let mut timed_out_response = Vec::new();
    timeout(
        Duration::from_secs(2),
        slow_client.read_to_end(&mut timed_out_response),
    )
    .await??;
    assert!(
        !timed_out_response
            .windows(6)
            .any(|bytes| bytes == b"200 OK")
    );
    let mut second_timeout = Vec::new();
    timeout(
        Duration::from_secs(2),
        second_slow_client.read_to_end(&mut second_timeout),
    )
    .await??;
    assert!(!second_timeout.windows(6).any(|bytes| bytes == b"200 OK"));

    let _idle_client = TcpStream::connect(address).await?;
    shutdown_sender
        .send(())
        .map_err(|()| "server stopped before shutdown request")?;
    timeout(Duration::from_secs(2), server).await???;
    Ok(())
}
