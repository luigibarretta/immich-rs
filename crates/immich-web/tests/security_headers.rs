#![forbid(unsafe_code)]

//! The Web Console response header policy on representative responses:
//! server-rendered HTML, static assets, the machine metrics API, redirects and
//! every class of rejection (request policy, routing, limits, authentication).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use tower::ServiceExt;

#[path = "security_headers/contract.rs"]
mod contract;
mod support;

use contract::assert_security_headers;
use support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

const METRICS_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn metrics(
    router: &Router,
    authorization: Option<&str>,
) -> Result<(StatusCode, HeaderMap), Box<dyn std::error::Error>> {
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri("/metrics")
        .header("host", HOST);
    if let Some(value) = authorization {
        builder = builder.header("authorization", value);
    }
    let mut request = builder.body(Body::empty())?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        40_000,
    )));
    let response = router.clone().oneshot(request).await?;
    let (parts, body) = response.into_parts();
    to_bytes(body, 128 * 1_024).await?;
    Ok((parts.status, parts.headers))
}

#[tokio::test]
async fn successful_responses_carry_the_complete_policy() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace = TestWorkspace::new()?;
    let router = workspace.console()?.router();

    let page = send(&router, Method::GET, "/pair", Some(HOST), None, None, "").await?;
    assert_eq!(page.status, StatusCode::OK);
    assert_security_headers(&page.headers, "HTML pairing page");

    for asset in ["/assets/console.css", "/assets/job.js"] {
        let response = send(&router, Method::GET, asset, Some(HOST), None, None, "").await?;
        assert_eq!(response.status, StatusCode::OK, "{asset}");
        assert_security_headers(&response.headers, asset);
    }

    let pairing_cookie = response_cookie(&page.headers, "immich_rs_pairing")?;
    let paired = send(
        &router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &format!("csrf={}&secret={SECRET}", csrf(&page.body)?),
    )
    .await?;
    assert_eq!(paired.status, StatusCode::SEE_OTHER);
    assert_security_headers(&paired.headers, "pairing redirect");

    let (status, headers) = metrics(&router, Some(&format!("Bearer {METRICS_TOKEN}"))).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/plain"))
    );
    assert_security_headers(&headers, "machine metrics API");
    Ok(())
}

#[tokio::test]
async fn rejections_carry_the_complete_policy() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let router = workspace.console()?.router();

    let no_host = send(&router, Method::GET, "/", None, None, None, "").await?;
    assert_eq!(no_host.status, StatusCode::BAD_REQUEST);
    assert_security_headers(&no_host.headers, "request policy 400");

    let bad_origin = send(
        &router,
        Method::POST,
        "/logout",
        Some(HOST),
        Some("http://localhost:2285"),
        None,
        "csrf=synthetic",
    )
    .await?;
    assert_eq!(bad_origin.status, StatusCode::FORBIDDEN);
    assert_security_headers(&bad_origin.headers, "request policy 403");

    let missing = send(&router, Method::GET, "/missing", Some(HOST), None, None, "").await?;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_security_headers(&missing.headers, "fallback 404");

    let wrong_method = send(&router, Method::GET, "/logout", Some(HOST), None, None, "").await?;
    assert_eq!(wrong_method.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_security_headers(&wrong_method.headers, "method 405");

    let oversized = "x".repeat(17 * 1_024);
    let too_large = send(
        &router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        None,
        &oversized,
    )
    .await?;
    assert_eq!(too_large.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_security_headers(
        &too_large.headers,
        "declared Content-Length over the policy limit 413",
    );

    let locked = send(&router, Method::GET, "/history", Some(HOST), None, None, "").await?;
    assert_eq!(locked.status, StatusCode::UNAUTHORIZED);
    assert_security_headers(&locked.headers, "unauthenticated 401");

    let (status, headers) = metrics(&router, Some("Bearer synthetic-wrong")).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_security_headers(&headers, "metrics 401");
    Ok(())
}
