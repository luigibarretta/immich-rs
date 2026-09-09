use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_TYPE, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;
use tokio::sync::oneshot;

pub const SYNTHETIC_KEY: &str = "synthetic-api-key";

pub struct MockState {
    pub reads: AtomicUsize,
    pub checks: AtomicUsize,
    pub uploads: AtomicUsize,
    pub reject_known: AtomicBool,
    pub upload_response_delay_ms: AtomicU64,
    pub bulk_failures: AtomicUsize,
    pub upload_failures: AtomicUsize,
}

pub struct MockServer {
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl MockServer {
    pub async fn stop(mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(shutdown) = self.shutdown.take() {
            let _sent = shutdown.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.await??;
        }
        Ok(())
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _sent = shutdown.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn version(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    json_response(&json!({"major": 3, "minor": 1, "patch": 0, "prerelease": null}))
}

async fn user(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    json_response(&json!({"id": "00000000-0000-4000-8000-000000000001"}))
}

async fn bulk_check(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.checks.fetch_add(1, Ordering::Relaxed);
    if take_failure(&state.bulk_failures) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(id) = value
        .get("assets")
        .and_then(|assets| assets.get(0))
        .and_then(|asset| asset.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if state.reject_known.load(Ordering::Relaxed) && state.uploads.load(Ordering::Relaxed) > 0 {
        json_response(&json!({"results": [{
            "id": id,
            "action": "reject",
            "assetId": "00000000-0000-4000-8000-000000000010"
        }]}))
    } else {
        json_response(&json!({"results": [{"id": id, "action": "accept", "assetId": null}]}))
    }
}

async fn upload(State(state): State<Arc<MockState>>, headers: HeaderMap, _body: Bytes) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.uploads.fetch_add(1, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(
        state.upload_response_delay_ms.load(Ordering::Relaxed),
    ))
    .await;
    if take_failure(&state.upload_failures) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    json_response(&json!({
        "id": "00000000-0000-4000-8000-000000000010",
        "status": "created"
    }))
}

fn json_response(value: &serde_json::Value) -> Response {
    serde_json::to_vec(value).map_or_else(
        |_| StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        |body| ([(CONTENT_TYPE, "application/json")], Body::from(body)).into_response(),
    )
}

fn authorized(state: &MockState, headers: &HeaderMap) -> bool {
    let accepted = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        == Some(SYNTHETIC_KEY);
    if accepted {
        state.reads.fetch_add(1, Ordering::Relaxed);
    }
    accepted
}

fn take_failure(remaining: &AtomicUsize) -> bool {
    remaining
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_sub(1)
        })
        .is_ok()
}

pub async fn start_mock() -> Result<(String, Arc<MockState>, MockServer), Box<dyn std::error::Error>>
{
    let state = Arc::new(MockState {
        reads: AtomicUsize::new(0),
        checks: AtomicUsize::new(0),
        uploads: AtomicUsize::new(0),
        reject_known: AtomicBool::new(false),
        upload_response_delay_ms: AtomicU64::new(0),
        bulk_failures: AtomicUsize::new(0),
        upload_failures: AtomicUsize::new(0),
    });
    let router = Router::new()
        .route("/api/server/version", get(version))
        .route("/api/users/me", get(user))
        .route("/api/assets/bulk-upload-check", post(bulk_check))
        .route("/api/assets", post(upload))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let (shutdown_send, shutdown_receive) = oneshot::channel();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _shutdown = shutdown_receive.await;
            })
            .await
    });
    Ok((
        origin,
        state,
        MockServer {
            shutdown: Some(shutdown_send),
            handle: Some(handle),
        },
    ))
}
