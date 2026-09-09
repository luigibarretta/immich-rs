#![forbid(unsafe_code)]

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, SET_COOKIE};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use immich_rs_web::{WebConfig, WebConsole};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:2285";
const ORIGIN: &str = "http://127.0.0.1:2285";
const SECRET: &str = "synthetic-bootstrap-secret";
static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestWorkspace {
    root: PathBuf,
}

struct TestResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl TestWorkspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-web-http-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn console(&self) -> Result<WebConsole, Box<dyn std::error::Error>> {
        let source = self.path("source");
        fs::create_dir_all(&source)?;
        fs::write(source.join("synthetic.jpg"), b"synthetic\n")?;
        let secret_path = self.path("bootstrap.secret");
        fs::write(&secret_path, format!("{SECRET}\n"))?;
        make_private(&secret_path)?;
        let config_path = self.path("web.toml");
        fs::write(
            &config_path,
            configuration(
                self,
                &source,
                &secret_path,
                "Camera <script>alert(1)</script>",
            ),
        )?;
        Ok(WebConsole::from_config(WebConfig::load(&config_path)?)?)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

fn configuration(workspace: &TestWorkspace, source: &Path, secret: &Path, label: &str) -> String {
    format!(
        r#"schema_version = 1

[web]
listen_address = "127.0.0.1:2285"
public_origin = "http://127.0.0.1:2285"
bootstrap_secret_file = "{}"

[[sources]]
id = "camera_roll"
label = "{}"
allowed_root = "{}"
relative_root = "."
generation = 1

[[servers]]
id = "disposable"
origin = "http://127.0.0.1:9387"
api_key_file = "{}"
generation = 1
"#,
        secret.display(),
        label,
        source.display(),
        workspace.path("never-read-api-key.secret").display(),
    )
}

#[cfg(unix)]
fn make_private(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn make_private(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    host: Option<&str>,
    origin: Option<&str>,
    cookie: Option<&str>,
    body: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(value) = host {
        builder = builder.header("host", value);
    }
    if let Some(value) = origin {
        builder = builder.header("origin", value);
    }
    if let Some(value) = cookie {
        builder = builder.header("cookie", value);
    }
    if !body.is_empty() {
        builder = builder.header(CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    let mut request = builder.body(Body::from(body.to_owned()))?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        40_000,
    )));
    let response = router.clone().oneshot(request).await?;
    let (parts, response_body) = response.into_parts();
    let bytes = to_bytes(response_body, 128 * 1_024).await?;
    Ok(TestResponse {
        status: parts.status,
        headers: parts.headers,
        body: String::from_utf8(bytes.to_vec())?,
    })
}

fn csrf(body: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let marker = "name=\"csrf\" value=\"";
    let start = body.find(marker).ok_or("CSRF field missing")? + marker.len();
    let rest = body.get(start..).ok_or("CSRF start invalid")?;
    let end = rest.find('"').ok_or("CSRF value invalid")?;
    rest.get(..end).ok_or_else(|| "CSRF value missing".into())
}

fn response_cookie(headers: &HeaderMap, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    for header in headers.get_all(SET_COOKIE) {
        let value = header.to_str()?;
        if let Some(cookie) = value.split(';').next()
            && cookie.starts_with(&format!("{name}="))
            && !cookie.ends_with('=')
        {
            return Ok(cookie.to_owned());
        }
    }
    Err(format!("cookie {name} missing").into())
}

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
            .contains(&workspace.root.display().to_string())
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
