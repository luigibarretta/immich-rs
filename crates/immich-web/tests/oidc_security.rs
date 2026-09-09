#![forbid(unsafe_code)]

#[path = "oidc_security/idp.rs"]
mod idp;
#[path = "oidc_security/sse.rs"]
mod sse;
#[path = "oidc_security/support.rs"]
mod support;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::{HOST, ORIGIN};
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;
use url::Url;

use idp::{ClaimsMode, DisposableIdp};
use support::{
    HOST_VALUE, ORIGIN_VALUE, TestResponse, TestWorkspace, TlsIdentity, cookie, csrf, location,
    send,
};

struct LoginAttempt {
    callback: String,
}

#[tokio::test]
async fn oidc_code_pkce_issues_rotated_secure_session() -> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let console = workspace.console(&identity, &idp.issuer)?;
    let router = console.router();

    let first = begin(&router, &identity, None).await?;
    let response = finish(&router, &first).await?;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    let session = cookie(&response.headers, "immich_rs_session")?;
    let set_cookie = response
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("immich_rs_session="))
        .ok_or("session cookie missing")?;
    assert!(set_cookie.contains("; Secure"));
    assert!(set_cookie.contains("; HttpOnly"));
    assert!(set_cookie.contains("; SameSite=Strict"));
    let history = send(&router, Method::GET, "/history", None, Some(&session), "").await?;
    assert_eq!(history.status, StatusCode::OK);

    let second = begin(&router, &identity, Some(&session)).await?;
    let rotated = finish(&router, &second).await?;
    let replacement = cookie(&rotated.headers, "immich_rs_session")?;
    assert_ne!(session, replacement);
    let old = send(&router, Method::GET, "/history", None, Some(&session), "").await?;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    let current = send(
        &router,
        Method::GET,
        "/history",
        None,
        Some(&replacement),
        "",
    )
    .await?;
    assert_eq!(current.status, StatusCode::OK);
    idp.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn oidc_claim_policy_denies_invalid_tokens() -> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let router = workspace.console(&identity, &idp.issuer)?.router();
    for mode in [
        ClaimsMode::WrongIssuer,
        ClaimsMode::WrongAudience,
        ClaimsMode::WrongNonce,
        ClaimsMode::Expired,
        ClaimsMode::RoleDenied,
    ] {
        idp.state.set_claims(mode);
        let attempt = begin(&router, &identity, None).await?;
        let response = finish(&router, &attempt).await?;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    }
    idp.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn oidc_state_and_code_are_single_use() -> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let router = workspace.console(&identity, &idp.issuer)?.router();
    let attempt = begin(&router, &identity, None).await?;
    assert_eq!(
        finish(&router, &attempt).await?.status,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        finish(&router, &attempt).await?.status,
        StatusCode::UNAUTHORIZED
    );

    let first = begin(&router, &identity, None).await?;
    let second = begin(&router, &identity, None).await?;
    let first_url = Url::parse(&format!("{ORIGIN_VALUE}{}", first.callback))?;
    let second_url = Url::parse(&format!("{ORIGIN_VALUE}{}", second.callback))?;
    let first_code = query(&first_url, "code")?;
    let second_state = query(&second_url, "state")?;
    let replay = LoginAttempt {
        callback: format!("/oidc/callback?code={first_code}&state={second_state}"),
    };
    assert_eq!(
        finish(&router, &replay).await?.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        finish(&router, &first).await?.status,
        StatusCode::UNAUTHORIZED
    );
    idp.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn oidc_key_rotation_succeeds_and_outage_fails_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let router = workspace.console(&identity, &idp.issuer)?.router();
    let rotating = begin(&router, &identity, None).await?;
    idp.state.rotate()?;
    assert_eq!(
        finish(&router, &rotating).await?.status,
        StatusCode::SEE_OTHER
    );

    let outage = begin(&router, &identity, None).await?;
    idp.state.set_outage(true);
    assert_eq!(
        finish(&router, &outage).await?.status,
        StatusCode::UNAUTHORIZED
    );
    idp.state.set_outage(false);
    idp.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn lan_policy_rejects_origin_and_proxy_spoofing() -> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let router = workspace.console(&identity, &idp.issuer)?.router();
    let page = send(&router, Method::GET, "/", None, None, "").await?;
    let login_cookie = cookie(&page.headers, "immich_rs_oidc_login")?;
    let token = csrf(&page.body)?;
    let wrong_origin = send(
        &router,
        Method::POST,
        "/oidc/login",
        Some("https://evil.invalid"),
        Some(&login_cookie),
        &format!("csrf={token}"),
    )
    .await?;
    assert_eq!(wrong_origin.status, StatusCode::FORBIDDEN);

    let mut request = Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(HOST, HOST_VALUE)
        .header(ORIGIN, ORIGIN_VALUE)
        .header("x-forwarded-for", "127.0.0.1")
        .body(Body::empty())?;
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.40:45000".parse::<std::net::SocketAddr>()?,
    ));
    let response = router.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    idp.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn direct_tls_listener_serves_lan_console_and_shuts_down_cleanly()
-> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    let workspace = TestWorkspace::new()?;
    let console = workspace.console_on(&identity, &idp.issuer, port)?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(console.serve_on(listener, async {
        let _stop_result = stopped.await;
    }));
    let response = identity
        .client()?
        .get(format!("https://127.0.0.1:{port}/"))
        .header(HOST, format!("console.example:{port}"))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let _stop_result = stop.send(());
    server.await??;
    idp.shutdown().await;
    Ok(())
}

#[cfg(unix)]
#[test]
fn tls_key_must_be_private_regular_and_match_certificate() -> Result<(), Box<dyn std::error::Error>>
{
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let runtime = tokio::runtime::Runtime::new()?;
    let identity = TlsIdentity::new()?;
    let idp = runtime.block_on(DisposableIdp::start(&identity))?;
    let workspace = TestWorkspace::new()?;
    let config_path = workspace.prepare_config(&identity, &idp.issuer, 2_285)?;
    let key = workspace.path("console.key");
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644))?;
    assert!(
        immich_rs_web::WebConsole::from_config(immich_rs_web::WebConfig::load(&config_path)?)
            .is_err()
    );

    let second = TlsIdentity::new()?;
    fs::write(&key, &second.key_pem)?;
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600))?;
    assert!(
        immich_rs_web::WebConsole::from_config(immich_rs_web::WebConfig::load(&config_path)?)
            .is_err()
    );

    fs::remove_file(&key)?;
    symlink(workspace.path("console.crt"), &key)?;
    assert!(
        immich_rs_web::WebConsole::from_config(immich_rs_web::WebConfig::load(&config_path)?)
            .is_err()
    );
    runtime.block_on(idp.shutdown());
    Ok(())
}

#[tokio::test]
async fn idp_dns_outside_operator_cidr_fails_before_outbound_http()
-> Result<(), Box<dyn std::error::Error>> {
    let identity = TlsIdentity::new()?;
    let idp = DisposableIdp::start(&identity).await?;
    let workspace = TestWorkspace::new()?;
    let config_path = workspace.prepare_config(&identity, &idp.issuer, 2_285)?;
    let config = std::fs::read_to_string(&config_path)?.replace(
        "allowed_cidrs = [\"127.0.0.1/32\"]",
        "allowed_cidrs = [\"192.0.2.0/24\"]",
    );
    std::fs::write(&config_path, config)?;
    let router =
        immich_rs_web::WebConsole::from_config(immich_rs_web::WebConfig::load(&config_path)?)?
            .router();
    let page = send(&router, Method::GET, "/", None, None, "").await?;
    let login_cookie = cookie(&page.headers, "immich_rs_oidc_login")?;
    let token = csrf(&page.body)?;
    let response = send(
        &router,
        Method::POST,
        "/oidc/login",
        Some(ORIGIN_VALUE),
        Some(&login_cookie),
        &format!("csrf={token}"),
    )
    .await?;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    idp.shutdown().await;
    Ok(())
}

async fn begin(
    router: &Router,
    identity: &TlsIdentity,
    session: Option<&str>,
) -> Result<LoginAttempt, Box<dyn std::error::Error>> {
    let page_uri = if session.is_some() {
        "/oidc/login"
    } else {
        "/"
    };
    let page = send(router, Method::GET, page_uri, None, session, "").await?;
    assert_eq!(page.status, StatusCode::OK);
    let login_cookie = cookie(&page.headers, "immich_rs_oidc_login")?;
    let csrf = csrf(&page.body)?;
    let cookie_header = session.map_or_else(
        || login_cookie.clone(),
        |session| format!("{login_cookie}; {session}"),
    );
    let response = send(
        router,
        Method::POST,
        "/oidc/login",
        Some(ORIGIN_VALUE),
        Some(&cookie_header),
        &format!("csrf={csrf}"),
    )
    .await?;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    let authorization = location(&response.headers)?;
    let client = identity.client()?;
    let authorized = client.get(authorization).send().await?;
    assert_eq!(authorized.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    let callback = location(authorized.headers())?;
    let parsed = Url::parse(&callback)?;
    Ok(LoginAttempt {
        callback: parsed.path_and_query(),
    })
}

async fn finish(
    router: &Router,
    attempt: &LoginAttempt,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    send(router, Method::GET, &attempt.callback, None, None, "").await
}

fn query(url: &Url, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| "query value missing".into())
}

trait PathAndQuery {
    fn path_and_query(&self) -> String;
}

impl PathAndQuery for Url {
    fn path_and_query(&self) -> String {
        self.query().map_or_else(
            || self.path().to_owned(),
            |query| format!("{}?{query}", self.path()),
        )
    }
}
