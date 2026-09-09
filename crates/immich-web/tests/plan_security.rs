#![forbid(unsafe_code)]

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap, LOCATION};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use tokio::sync::oneshot;

mod support;

use support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

const SYNTHETIC_KEY: &str = "synthetic-api-key";

struct PairedSession {
    cookie: String,
    csrf: String,
}

struct MockState {
    authenticated_reads: AtomicUsize,
}

struct MockServer {
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl MockServer {
    async fn stop(mut self) -> Result<(), Box<dyn std::error::Error>> {
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
    json_response(&json!({
        "major": 3,
        "minor": 1,
        "patch": 0,
        "prerelease": null
    }))
}

async fn user(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    json_response(&json!({"id": "00000000-0000-4000-8000-000000000001"}))
}

fn json_response(value: &serde_json::Value) -> Response {
    serde_json::to_vec(value).map_or_else(
        |_| StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        |body| ([(CONTENT_TYPE, "application/json")], Body::from(body)).into_response(),
    )
}

fn authorized(state: &MockState, headers: &HeaderMap) -> bool {
    let authorized = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        == Some(SYNTHETIC_KEY);
    if authorized {
        state.authenticated_reads.fetch_add(1, Ordering::Relaxed);
    }
    authorized
}

async fn pair(router: &Router) -> Result<PairedSession, Box<dyn std::error::Error>> {
    let page = send(router, Method::GET, "/pair", Some(HOST), None, None, "").await?;
    let pairing_cookie = response_cookie(&page.headers, "immich_rs_pairing")?;
    let pairing_csrf = csrf(&page.body)?.to_owned();
    let response = send(
        router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    let cookie = response_cookie(&response.headers, "immich_rs_session")?;
    let dashboard = send(
        router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&cookie),
        "",
    )
    .await?;
    Ok(PairedSession {
        cookie,
        csrf: csrf(&dashboard.body)?.to_owned(),
    })
}

async fn wait_for_plan(
    router: &Router,
    session: &PairedSession,
    location: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    for _attempt in 0..200 {
        let response = send(
            router,
            Method::GET,
            location,
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        if response.body.contains("/plans/") {
            return extract_plan_ref(&response.body);
        }
        if response.body.contains("Failed ·") {
            return Err("server-bound planning failed".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err("server-bound planning did not finish".into())
}

fn extract_plan_ref(body: &str) -> Result<String, Box<dyn std::error::Error>> {
    let marker = "href=\"/plans/";
    let start = body.find(marker).ok_or("plan reference missing")? + marker.len();
    let remaining = body.get(start..).ok_or("plan reference start missing")?;
    let end = remaining.find('"').ok_or("plan reference end missing")?;
    remaining
        .get(..end)
        .map(str::to_owned)
        .ok_or_else(|| "plan reference missing".into())
}

#[cfg(unix)]
fn make_private(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn make_private(_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[tokio::test]
async fn authenticated_probe_publishes_private_inspectable_streamed_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let mock_state = Arc::new(MockState {
        authenticated_reads: AtomicUsize::new(0),
    });
    let mock = Router::new()
        .route("/api/server/version", get(version))
        .route("/api/users/me", get(user))
        .with_state(Arc::clone(&mock_state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let (shutdown_send, shutdown_receive) = oneshot::channel();
    let handle = tokio::spawn(async move {
        axum::serve(listener, mock)
            .with_graceful_shutdown(async move {
                let _shutdown = shutdown_receive.await;
            })
            .await
    });
    let server = MockServer {
        shutdown: Some(shutdown_send),
        handle: Some(handle),
    };

    let workspace = TestWorkspace::new()?;
    drop(workspace.console()?);
    let key_path = workspace.path("never-read-api-key.secret");
    fs::write(&key_path, format!("{SYNTHETIC_KEY}\n"))?;
    make_private(&key_path)?;
    let router = workspace.console_with_server(&origin)?.router();
    let session = pair(&router).await?;
    let response = send(
        &router,
        Method::POST,
        "/sources/camera_roll/servers/disposable/plan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    let job = response
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("job location missing")?;
    let plan_ref = wait_for_plan(&router, &session, job).await?;
    assert_eq!(mock_state.authenticated_reads.load(Ordering::Relaxed), 2);

    let locked = send(
        &router,
        Method::GET,
        &format!("/plans/{plan_ref}"),
        Some(HOST),
        None,
        None,
        "",
    )
    .await?;
    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    let inspection = send(
        &router,
        Method::GET,
        &format!("/plans/{plan_ref}"),
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(inspection.status, StatusCode::OK);
    assert!(inspection.body.contains("Canonical plan digest"));
    assert!(!inspection.body.contains(&origin));
    assert!(
        !inspection
            .body
            .contains(&workspace.path("").display().to_string())
    );

    let export = send(
        &router,
        Method::GET,
        &format!("/plans/{plan_ref}/export"),
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(export.status, StatusCode::OK);
    assert_eq!(
        export
            .headers
            .get(CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=immich-rs-plan.json")
    );
    let exported: immich_rs_application::UploadPlan = serde_json::from_str(&export.body)?;
    exported.validate()?;

    server.stop().await?;
    Ok(())
}
