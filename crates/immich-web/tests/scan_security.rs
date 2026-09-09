#![forbid(unsafe_code)]

use std::fs;
use std::io::ErrorKind;
use std::net::TcpListener;

use axum::Router;
use axum::http::header::LOCATION;
use axum::http::{Method, StatusCode};

mod support;

use support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

struct PairedSession {
    cookie: String,
    csrf: String,
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
    if response.status != StatusCode::SEE_OTHER {
        return Err("pairing did not redirect".into());
    }
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

async fn wait_for_job(
    router: &Router,
    session: &PairedSession,
    location: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    for _attempt in 0..100 {
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
        if response.status != StatusCode::OK {
            return Err("job status request failed".into());
        }
        if response.body.contains("Completed ·") || response.body.contains("Failed ·") {
            return Ok(response.body);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Err("job did not finish within the bounded wait".into())
}

#[tokio::test]
async fn scan_uses_only_authenticated_opaque_profile_and_never_connects()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let console = workspace.console_with_server(&format!("http://{}", listener.local_addr()?))?;
    let router = console.router();

    let unauthenticated = send(
        &router,
        Method::POST,
        "/sources/camera_roll/scan",
        Some(HOST),
        Some(ORIGIN),
        None,
        "csrf=synthetic-invalid",
    )
    .await?;
    assert_eq!(unauthenticated.status, StatusCode::UNAUTHORIZED);

    let session = pair(&router).await?;
    let mutation_get = send(
        &router,
        Method::GET,
        "/sources/camera_roll/scan",
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(mutation_get.status, StatusCode::METHOD_NOT_ALLOWED);

    let arbitrary_path = send(
        &router,
        Method::POST,
        "/sources/camera_roll/scan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}&path=/etc", session.csrf),
    )
    .await?;
    assert_eq!(arbitrary_path.status, StatusCode::UNPROCESSABLE_ENTITY);

    let unknown = send(
        &router,
        Method::POST,
        "/sources/%2e%2e/scan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);

    let scanned = send(
        &router,
        Method::POST,
        "/sources/camera_roll/scan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(scanned.status, StatusCode::SEE_OTHER);
    let location = scanned
        .headers
        .get(LOCATION)
        .ok_or("job redirect missing")?
        .to_str()?;
    let body = wait_for_job(&router, &session, location).await?;
    assert!(body.contains("Completed ·"));
    assert!(body.contains("Completed plan summary"));
    assert!(body.contains("<dd>1</dd>"));
    assert!(!body.contains("synthetic.jpg"));
    assert!(!body.contains(&workspace.path("").display().to_string()));
    assert!(!workspace.path("never-read-api-key.secret").exists());
    match listener.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        _ => return Err("source-only scan attempted an outbound connection".into()),
    }
    Ok(())
}

#[tokio::test]
async fn source_identity_swap_fails_before_scan_and_returns_no_path()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let console = workspace.console()?;
    let router = console.router();
    let session = pair(&router).await?;
    let source = workspace.path("source");
    fs::rename(&source, workspace.path("parked"))?;
    fs::create_dir(&source)?;

    let response = send(
        &router,
        Method::POST,
        "/sources/camera_roll/scan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    let location = response
        .headers
        .get(LOCATION)
        .ok_or("job redirect missing")?
        .to_str()?;
    let body = wait_for_job(&router, &session, location).await?;
    assert!(body.contains("Failed ·"));
    assert!(!body.contains("Completed plan summary"));
    assert!(!body.contains(&source.display().to_string()));
    Ok(())
}
