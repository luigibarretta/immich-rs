#![forbid(unsafe_code)]

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, SET_COOKIE};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use immich_rs_web::{WebConfig, WebConsole};
use tower::ServiceExt;

pub const HOST: &str = "127.0.0.1:2285";
pub const ORIGIN: &str = "http://127.0.0.1:2285";
pub const SECRET: &str = "synthetic-bootstrap-secret";
static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct TestWorkspace {
    root: PathBuf,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl TestWorkspace {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-web-http-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        fs::create_dir(root.join("state"))?;
        make_private_directory(&root.join("state"))?;
        Ok(Self { root })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn console(&self) -> Result<WebConsole, Box<dyn std::error::Error>> {
        self.console_with_server("http://127.0.0.1:9387")
    }

    pub fn console_with_server(
        &self,
        server_origin: &str,
    ) -> Result<WebConsole, Box<dyn std::error::Error>> {
        self.console_on(2_285, server_origin)
    }

    pub fn console_on(
        &self,
        port: u16,
        server_origin: &str,
    ) -> Result<WebConsole, Box<dyn std::error::Error>> {
        let source = self.path("source");
        fs::create_dir_all(&source)?;
        fs::write(source.join("synthetic.jpg"), b"synthetic\n")?;
        let bootstrap_path = self.path("bootstrap.secret");
        fs::write(&bootstrap_path, format!("{SECRET}\n"))?;
        make_private(&bootstrap_path)?;
        let config_path = self.path("web.toml");
        fs::write(
            &config_path,
            configuration(
                self,
                &source,
                &bootstrap_path,
                port,
                server_origin,
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

fn configuration(
    workspace: &TestWorkspace,
    source: &Path,
    bootstrap_path: &Path,
    port: u16,
    server_origin: &str,
    label: &str,
) -> String {
    format!(
        r#"schema_version = 1

[web]
listen_address = "127.0.0.1:{}"
public_origin = "http://127.0.0.1:{}"
bootstrap_secret_file = "{}"
history_state_id = "console"

[web.limits]
request_header_bytes = 1024
accepted_connections = 2
header_read_seconds = 1

[[sources]]
id = "camera_roll"
label = "{}"
allowed_root = "{}"
relative_root = "."
generation = 1

[[servers]]
id = "disposable"
origin = "{}"
api_key_file = "{}"
mode = "disposable"
generation = 1
credential_generation = 1

[[states]]
id = "console"
label = "Synthetic console state"
allowed_root = "{}"
relative_root = "state"
generation = 1
"#,
        port,
        port,
        bootstrap_path.display(),
        label,
        source.display(),
        server_origin,
        workspace.path("never-read-api-key.secret").display(),
        workspace.path("").display(),
    )
}

#[cfg(unix)]
fn make_private(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn make_private_directory(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(windows)]
fn make_private(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(windows)]
fn make_private_directory(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

pub async fn send(
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

pub fn csrf(body: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let marker = "name=\"csrf\" value=\"";
    let start = body.find(marker).ok_or("CSRF field missing")? + marker.len();
    let rest = body.get(start..).ok_or("CSRF start invalid")?;
    let end = rest.find('"').ok_or("CSRF value invalid")?;
    rest.get(..end).ok_or_else(|| "CSRF value missing".into())
}

pub fn response_cookie(
    headers: &HeaderMap,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
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
