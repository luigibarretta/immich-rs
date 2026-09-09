#![forbid(unsafe_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::CONTENT_SECURITY_POLICY;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

mod support;

use support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

#[tokio::test]
async fn pairing_authenticates_rotates_and_logs_out_with_strict_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let console = workspace.console()?;
    let router = console.router();

    let no_host = send(&router, Method::GET, "/", None, None, None, "").await?;
    assert_eq!(no_host.status, StatusCode::BAD_REQUEST);
    assert!(no_host.headers.contains_key(CONTENT_SECURITY_POLICY));

    let pair_page = send(&router, Method::GET, "/", Some(HOST), None, None, "").await?;
    assert_eq!(pair_page.status, StatusCode::OK);
    let pairing_cookie = response_cookie(&pair_page.headers, "immich_rs_pairing")?;
    let pairing_csrf = csrf(&pair_page.body)?.to_owned();
    assert!(!pair_page.body.contains(SECRET));

    let bad_origin = send(
        &router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some("http://localhost:2285"),
        Some(&pairing_cookie),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    assert_eq!(bad_origin.status, StatusCode::FORBIDDEN);

    let fixed = format!("{pairing_cookie}; immich_rs_session=fixed");
    let paired = send(
        &router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&fixed),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    assert_eq!(paired.status, StatusCode::SEE_OTHER);
    let session_cookie = response_cookie(&paired.headers, "immich_rs_session")?;
    assert_ne!(session_cookie, "immich_rs_session=fixed");

    let dashboard = send(
        &router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&session_cookie),
        "",
    )
    .await?;
    assert_eq!(dashboard.status, StatusCode::OK);
    assert!(
        dashboard
            .body
            .contains("Camera &#60;script&#62;alert(1)&#60;/script&#62;")
    );
    assert!(!dashboard.body.contains("<script>alert(1)</script>"));
    assert!(
        !dashboard
            .body
            .contains(&workspace.path("").display().to_string())
    );
    assert!(!dashboard.body.contains("127.0.0.1:9387"));
    let logout_csrf = csrf(&dashboard.body)?.to_owned();

    let mutation_get = send(
        &router,
        Method::GET,
        "/logout",
        Some(HOST),
        None,
        Some(&session_cookie),
        "",
    )
    .await?;
    assert_eq!(mutation_get.status, StatusCode::METHOD_NOT_ALLOWED);

    let logout = send(
        &router,
        Method::POST,
        "/logout",
        Some(HOST),
        Some(ORIGIN),
        Some(&session_cookie),
        &format!("csrf={logout_csrf}"),
    )
    .await?;
    assert_eq!(logout.status, StatusCode::SEE_OTHER);
    let after_logout = send(
        &router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&session_cookie),
        "",
    )
    .await?;
    assert_eq!(after_logout.status, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn forwarded_headers_rate_limit_body_limit_and_restart_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let console = workspace.console()?;
    let router = console.router();
    let mut forwarded_request = Request::builder()
        .uri("/")
        .header("host", HOST)
        .header("x-forwarded-proto", "https")
        .body(Body::empty())?;
    forwarded_request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_001,
        )));
    let forwarded_response = router.clone().oneshot(forwarded_request).await?;
    assert_eq!(forwarded_response.status(), StatusCode::BAD_REQUEST);

    let page = send(&router, Method::GET, "/pair", Some(HOST), None, None, "").await?;
    let pairing_cookie = response_cookie(&page.headers, "immich_rs_pairing")?;
    let pairing_csrf = csrf(&page.body)?.to_owned();
    for attempt in 0..5 {
        let denied = send(
            &router,
            Method::POST,
            "/pair",
            Some(HOST),
            Some(ORIGIN),
            Some(&pairing_cookie),
            &format!("csrf={pairing_csrf}&secret=synthetic-wrong-{attempt}"),
        )
        .await?;
        assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
    }
    let limited = send(
        &router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);

    let oversized = "x".repeat(17 * 1_024);
    let too_large = send(
        &router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &oversized,
    )
    .await?;
    assert_eq!(too_large.status, StatusCode::PAYLOAD_TOO_LARGE);

    let restarted = workspace.console()?.router();
    let old_pairing = send(
        &restarted,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    assert_eq!(old_pairing.status, StatusCode::FORBIDDEN);
    Ok(())
}
