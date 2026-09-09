use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{HOST, LOCATION};
use axum::http::{Method, Request, StatusCode};
use futures_util::StreamExt;
use tower::ServiceExt;

use super::idp::DisposableIdp;
use super::support::{HOST_VALUE, ORIGIN_VALUE, TestWorkspace, TlsIdentity, cookie, csrf, send};
use super::{begin, finish};

#[tokio::test]
async fn oidc_session_rotation_revokes_open_sse_stream() -> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let router = workspace.console(&identity, &idp.issuer)?.router();
    let attempt = begin(&router, &identity, None).await?;
    let logged_in = finish(&router, &attempt).await?;
    let session = cookie(&logged_in.headers, "immich_rs_session")?;
    let dashboard = send(&router, Method::GET, "/", None, Some(&session), "").await?;
    let csrf = csrf(&dashboard.body)?;
    let admitted = send(
        &router,
        Method::POST,
        "/sources/source/scan",
        Some(ORIGIN_VALUE),
        Some(&session),
        &format!("csrf={csrf}"),
    )
    .await?;
    assert_eq!(admitted.status, StatusCode::SEE_OTHER);
    let location = admitted
        .headers
        .get(LOCATION)
        .ok_or("scan location missing")?
        .to_str()?;
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(format!("{location}/events"))
        .header(HOST, HOST_VALUE)
        .header("cookie", &session)
        .body(Body::empty())?;
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.40:45000".parse::<std::net::SocketAddr>()?,
    ));
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await?;
    assert!(first.is_some());

    let rotation = begin(&router, &identity, Some(&session)).await?;
    let rotated = finish(&router, &rotation).await?;
    assert_eq!(rotated.status, StatusCode::SEE_OTHER);
    let after = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next()).await?;
    assert!(after.is_none());
    idp.shutdown().await;
    Ok(())
}
