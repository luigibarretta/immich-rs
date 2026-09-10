use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, HOST, LOCATION, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use immich_rs_web::{WebConfig, WebConsole};
use rcgen::{CertificateParams, KeyPair, SanType};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;

pub const HOST_VALUE: &str = "console.example:2285";
pub const ORIGIN_VALUE: &str = "https://console.example:2285";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct TlsIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub cert_pem: String,
    pub key_pem: String,
}

pub struct TestWorkspace {
    root: PathBuf,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl TlsIdentity {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut params = CertificateParams::new(vec!["console.example".to_owned()])?;
        params
            .subject_alt_names
            .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        let key = KeyPair::generate()?;
        let certificate = params.self_signed(&key)?;
        Ok(Self {
            cert_der: certificate.der().to_vec(),
            key_der: key.serialize_der(),
            cert_pem: certificate.pem(),
            key_pem: key.serialize_pem(),
        })
    }

    pub fn acceptor(&self) -> Result<TlsAcceptor, Box<dyn std::error::Error>> {
        let certificates = vec![CertificateDer::from(self.cert_der.clone())];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(certificates, key)?;
        Ok(TlsAcceptor::from(Arc::new(server)))
    }

    pub fn client(&self) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
        let certificate = reqwest::Certificate::from_pem(self.cert_pem.as_bytes())?;
        Ok(reqwest::Client::builder()
            .add_root_certificate(certificate)
            .redirect(reqwest::redirect::Policy::none())
            .build()?)
    }
}

impl TestWorkspace {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-web-oidc-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        for directory in ["source", "state"] {
            fs::create_dir(root.join(directory))?;
        }
        make_private_directory(&root.join("state"))?;
        Ok(Self { root })
    }

    pub fn console(
        &self,
        identity: &TlsIdentity,
        issuer: &str,
    ) -> Result<WebConsole, Box<dyn std::error::Error>> {
        self.console_on(identity, issuer, 2_285)
    }

    pub fn console_on(
        &self,
        identity: &TlsIdentity,
        issuer: &str,
        port: u16,
    ) -> Result<WebConsole, Box<dyn std::error::Error>> {
        let path = self.prepare_config(identity, issuer, port)?;
        Ok(WebConsole::from_config(WebConfig::load(&path)?)?)
    }

    pub fn prepare_config(
        &self,
        identity: &TlsIdentity,
        issuer: &str,
        port: u16,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let certificate = self.path("console.crt");
        let key = self.path("console.key");
        let ca = self.path("idp-ca.crt");
        let secret = self.path("oidc.secret");
        fs::write(&certificate, &identity.cert_pem)?;
        fs::write(&key, &identity.key_pem)?;
        fs::write(&ca, &identity.cert_pem)?;
        fs::write(&secret, "synthetic-oidc-secret\n")?;
        make_private(&key)?;
        make_private(&secret)?;
        let config = format!(
            r#"schema_version = 1

[web]
listen_address = "0.0.0.0:{port}"
public_origin = "https://console.example:{port}"
history_state_id = "console"

[web.lan]
tls_certificate_file = {}
tls_private_key_file = {}

[web.lan.oidc]
issuer = "{issuer}"
client_id = "immich-rs-web"
client_secret_file = {}
ca_certificate_file = {}
allowed_cidrs = ["127.0.0.1/32"]
required_role = "operator"
allowed_algorithm = "EdDSA"

[[sources]]
id = "source"
label = "Synthetic source"
allowed_root = {}
relative_root = "."
generation = 1

[[servers]]
id = "disposable"
origin = "http://127.0.0.1:9387"
api_key_file = {}
mode = "disposable"
generation = 1
credential_generation = 1

[[states]]
id = "console"
label = "Synthetic state"
allowed_root = {}
relative_root = "state"
generation = 1
"#,
            toml_path(&certificate),
            toml_path(&key),
            toml_path(&secret),
            toml_path(&ca),
            toml_path(&self.path("source")),
            toml_path(&self.path("never-read-api-key.secret")),
            toml_path(&self.root),
        );
        let path = self.path("web.toml");
        fs::write(&path, config)?;
        Ok(path)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

fn toml_path(path: &Path) -> String {
    serde_json::Value::String(path.to_string_lossy().into_owned()).to_string()
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

pub async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
    body: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(uri)
        .header(HOST, HOST_VALUE);
    if let Some(origin) = origin {
        builder = builder.header(ORIGIN, origin);
    }
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if method == Method::POST {
        builder = builder.header(CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    let mut request = builder.body(Body::from(body.to_owned()))?;
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(10, 20, 30, 40)),
        45_000,
    )));
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok(TestResponse {
        status,
        headers,
        body: String::from_utf8(bytes.to_vec())?,
    })
}

pub fn csrf(body: &str) -> Result<String, Box<dyn std::error::Error>> {
    let marker = "name=\"csrf\" value=\"";
    let rest = body.split_once(marker).ok_or("CSRF marker missing")?.1;
    Ok(rest
        .split_once('"')
        .ok_or("CSRF value missing")?
        .0
        .to_owned())
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let prefix = format!("{name}=");
    for header in headers.get_all(SET_COOKIE) {
        let value = header.to_str()?;
        if let Some(rest) = value.strip_prefix(&prefix) {
            let token = rest.split_once(';').ok_or("cookie terminator missing")?.0;
            return Ok(format!("{name}={token}"));
        }
    }
    Err("cookie missing".into())
}

pub fn location(headers: &HeaderMap) -> Result<String, Box<dyn std::error::Error>> {
    Ok(headers
        .get(LOCATION)
        .ok_or("location missing")?
        .to_str()?
        .to_owned())
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
