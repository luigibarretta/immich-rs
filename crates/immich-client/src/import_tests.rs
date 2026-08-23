use std::error::Error;
use std::io;

use immich_rs_core::{
    CancellationToken, GeoCoordinates, NormalizedMetadata, ServerCompatibility, ServerVersion,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::{ApiKey, ClientConfig, ClientErrorClass, ImmichEndpoint, ImmichReadClient};

const ASSET_A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ASSET_B: &str = "bbbbbbbb-bbbb-4bbb-9bbb-bbbbbbbbbbbb";
const ALBUM: &str = "cccccccc-cccc-4ccc-accc-cccccccccccc";

#[derive(Debug)]
struct CapturedRequest {
    head: String,
    body: String,
}

#[tokio::test]
async fn import_capability_emits_exact_bounded_effects() -> Result<(), Box<dyn Error + Send + Sync>>
{
    let responses = vec![
        (200, format!(r#"{{"id":"{ASSET_A}"}}"#)),
        (
            200,
            format!(r#"[{{"id":"{ALBUM}","albumName":"Synthetic Album"}}]"#),
        ),
        (
            201,
            format!(r#"{{"id":"{ALBUM}","albumName":"Synthetic Album"}}"#),
        ),
        (
            200,
            format!(
                r#"[{{"id":"{ASSET_A}","success":true}},{{"id":"{ASSET_B}","success":false,"error":"duplicate"}}]"#
            ),
        ),
    ];
    let (origin, mut requests, server) = mock_server(responses).await?;
    let client = import_client(&origin)?;
    let cancellation = CancellationToken::default();
    let metadata = NormalizedMetadata {
        description: Some("Synthetic description".to_owned()),
        taken_at_utc: Some("2024-01-02T03:04:05Z".to_owned()),
        location: Some(GeoCoordinates {
            latitude: "1.5".to_owned(),
            longitude: "-2.25".to_owned(),
        }),
        albums: vec!["Synthetic Album".to_owned()],
    };

    client
        .update_asset_metadata(ASSET_A, &metadata, &cancellation)
        .await?;
    let albums = client
        .find_owned_albums("Synthetic Album", &cancellation)
        .await?;
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].id(), ALBUM);
    assert_eq!(albums[0].name(), "Synthetic Album");
    let created = client
        .create_album("Synthetic Album", &cancellation)
        .await?;
    assert_eq!(created.id(), ALBUM);
    client
        .add_album_assets(ALBUM, &[ASSET_A, ASSET_B], &cancellation)
        .await?;

    server.await??;
    let captured = receive_all(&mut requests).await;
    assert_eq!(captured.len(), 4);
    assert!(
        captured[0]
            .head
            .starts_with(&format!("PUT /api/assets/{ASSET_A} HTTP/1.1"))
    );
    assert_eq!(
        captured[0].body,
        r#"{"dateTimeOriginal":"2024-01-02T03:04:05Z","description":"Synthetic description","latitude":1.5,"longitude":-2.25}"#
    );
    assert!(
        captured[1]
            .head
            .starts_with("GET /api/albums?name=Synthetic+Album&isOwned=true HTTP/1.1")
    );
    assert!(captured[2].head.starts_with("POST /api/albums HTTP/1.1"));
    assert_eq!(captured[2].body, r#"{"albumName":"Synthetic Album"}"#);
    assert!(
        captured[3]
            .head
            .starts_with(&format!("PUT /api/albums/{ALBUM}/assets HTTP/1.1"))
    );
    assert_eq!(
        captured[3].body,
        format!(r#"{{"ids":["{ASSET_A}","{ASSET_B}"]}}"#)
    );
    Ok(())
}

#[tokio::test]
async fn invalid_or_cancelled_import_effects_never_reach_network()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let client = import_client("http://127.0.0.1:9")?;
    let cancellation = CancellationToken::default();
    let empty = NormalizedMetadata::default();
    let error = client
        .update_asset_metadata(ASSET_A, &empty, &cancellation)
        .await
        .err()
        .ok_or("empty metadata unexpectedly accepted")?;
    assert_eq!(error.class(), ClientErrorClass::Protocol);
    let error = client
        .add_album_assets(ALBUM, &[ASSET_B, ASSET_A], &cancellation)
        .await
        .err()
        .ok_or("unsorted IDs unexpectedly accepted")?;
    assert_eq!(error.class(), ClientErrorClass::Protocol);

    cancellation.cancel();
    let error = client
        .find_owned_albums("Synthetic Album", &cancellation)
        .await
        .err()
        .ok_or("cancelled request unexpectedly accepted")?;
    assert_eq!(error.class(), ClientErrorClass::Cancelled);
    Ok(())
}

fn import_client(origin: &str) -> Result<crate::ImmichImportClient, Box<dyn Error + Send + Sync>> {
    let endpoint = ImmichEndpoint::parse(origin)?;
    let origin_sha256 = format!(
        "{:x}",
        Sha256::digest(endpoint.canonical_origin().as_bytes())
    );
    let client = ImmichReadClient::new(
        endpoint,
        ApiKey::new("synthetic-test-key")?,
        ClientConfig::default(),
    )?;
    let negotiated = crate::NegotiatedServer::synthetic_for_test(
        ServerCompatibility {
            version: ServerVersion {
                major: 3,
                minor: 1,
                patch: 0,
            },
            identity_sha256: "d".repeat(64),
        },
        origin_sha256,
    );
    Ok(client.authorize_import(negotiated)?)
}

async fn mock_server(
    responses: Vec<(u16, String)>,
) -> io::Result<(
    String,
    mpsc::Receiver<CapturedRequest>,
    tokio::task::JoinHandle<io::Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(responses.len());
    let server = tokio::spawn(async move {
        for (status, response_body) in responses {
            let (mut stream, _) = listener.accept().await?;
            let request = read_request(&mut stream).await?;
            sender
                .send(request)
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "capture closed"))?;
            let reason = if status == 201 { "Created" } else { "OK" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            );
            stream.write_all(response.as_bytes()).await?;
        }
        Ok(())
    });
    Ok((format!("http://{address}"), receiver, server))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> io::Result<CapturedRequest> {
    let mut bytes = Vec::with_capacity(2_048);
    let mut chunk = [0_u8; 1_024];
    let (header_end, content_length) = loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "request"));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > 32 * 1_024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large",
            ));
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = end + 4;
            let head = std::str::from_utf8(&bytes[..header_end])
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request head"))?;
            let length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .map_or(Ok(0), |value| {
                    value
                        .parse::<usize>()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "content length"))
                })?;
            break (header_end, length);
        }
    };
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "request body"));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let head = String::from_utf8(bytes[..header_end].to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request head"))?;
    let body = String::from_utf8(bytes[header_end..header_end + content_length].to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request body"))?;
    Ok(CapturedRequest { head, body })
}

async fn receive_all(receiver: &mut mpsc::Receiver<CapturedRequest>) -> Vec<CapturedRequest> {
    let mut requests = Vec::new();
    while let Some(request) = receiver.recv().await {
        requests.push(request);
    }
    requests
}
