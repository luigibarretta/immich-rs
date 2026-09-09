#![forbid(unsafe_code)]

use axum::Router;
use axum::http::header::{CONTENT_TYPE, LOCATION};
use axum::http::{Method, StatusCode};

mod support;

use support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

struct Session {
    cookie: String,
    csrf: String,
}

async fn pair(router: &Router) -> Result<Session, Box<dyn std::error::Error>> {
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
    Ok(Session {
        cookie,
        csrf: csrf(&dashboard.body)?.to_owned(),
    })
}

async fn complete_scan(
    router: &Router,
    session: &Session,
) -> Result<(), Box<dyn std::error::Error>> {
    let admitted = send(
        router,
        Method::POST,
        "/sources/camera_roll/scan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    if admitted.status != StatusCode::SEE_OTHER {
        return Err("scan admission did not redirect".into());
    }
    let location = admitted
        .headers
        .get(LOCATION)
        .ok_or("scan redirect missing")?
        .to_str()?;
    for _attempt in 0..100 {
        let status = send(
            router,
            Method::GET,
            location,
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        if status.body.contains("Completed ·") {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Err("scan did not finish within the bounded wait".into())
}

#[tokio::test]
async fn metrics_are_authenticated_aggregate_and_label_free()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let router = workspace.console()?.router();
    let locked = send(&router, Method::GET, "/metrics", Some(HOST), None, None, "").await?;
    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    assert!(!locked.body.contains("immich_rs_web_sessions_active"));

    let session = pair(&router).await?;
    let metrics = send(
        &router,
        Method::GET,
        "/metrics",
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(metrics.status, StatusCode::OK);
    assert_eq!(
        metrics
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    assert!(metrics.body.len() < 4_096);
    assert!(metrics.body.contains("immich_rs_web_sessions_active 1\n"));
    assert!(metrics.body.contains("immich_rs_web_jobs_retained 0\n"));
    assert!(metrics.body.contains("immich_rs_web_history_rows 0\n"));
    assert!(!metrics.body.contains('{'));
    for forbidden in [
        "synthetic.jpg",
        "camera_roll",
        "127.0.0.1:9387",
        "sha256",
        "job_id",
        "user_id",
        "session_id",
        "metadata",
    ] {
        assert!(!metrics.body.contains(forbidden));
    }
    assert!(
        !metrics
            .body
            .contains(&workspace.path("").display().to_string())
    );

    complete_scan(&router, &session).await?;
    let after_scan = send(
        &router,
        Method::GET,
        "/metrics",
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(after_scan.status, StatusCode::OK);
    assert!(after_scan.body.contains("immich_rs_web_jobs_completed 1\n"));
    assert!(after_scan.body.contains("immich_rs_web_jobs_retained 1\n"));
    assert!(after_scan.body.contains("immich_rs_web_history_rows 1\n"));
    assert!(!after_scan.body.contains('{'));

    let logout = send(
        &router,
        Method::POST,
        "/logout",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(logout.status, StatusCode::SEE_OTHER);
    let revoked = send(
        &router,
        Method::GET,
        "/metrics",
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED);
    assert!(!revoked.body.contains("immich_rs_web_sessions_active"));
    Ok(())
}
