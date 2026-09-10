use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::http::HeaderMap;

use super::{MetricsAuthConfig, MetricsAuthenticator, RawMetricsAuthConfig};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "immich-rs-metrics-auth-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn token(&self, value: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = self.0.join("metrics.secret");
        fs::write(&path, format!("{value}\n"))?;
        make_private(&path)?;
        Ok(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

fn config(path: PathBuf, ranges: &[&str]) -> Result<MetricsAuthConfig, crate::WebConfigError> {
    MetricsAuthConfig::from_raw(RawMetricsAuthConfig {
        bearer_token_file: path,
        allowed_cidrs: ranges.iter().map(ToString::to_string).collect(),
    })
}

#[test]
fn exact_bearer_and_immediate_peer_are_both_required() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let auth = MetricsAuthenticator::load(&config(
        workspace.token(TOKEN)?,
        &["127.0.0.1/32", "fd42::/64"],
    )?)?;
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {TOKEN}").parse()?);
    assert!(auth.authorize(IpAddr::V4(Ipv4Addr::LOCALHOST), &headers));
    assert!(!auth.authorize("192.0.2.10".parse()?, &headers));

    headers.append("authorization", format!("Bearer {TOKEN}").parse()?);
    assert!(!auth.authorize(IpAddr::V4(Ipv4Addr::LOCALHOST), &headers));
    Ok(())
}

#[test]
fn malformed_policy_and_credentials_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let token = workspace.token(TOKEN)?;
    for ranges in [vec![], vec!["bad"], vec!["127.0.0.1/24", "127.0.0.0/24"]] {
        assert!(config(token.clone(), &ranges).is_err());
    }
    assert!(config(PathBuf::from("relative.secret"), &["127.0.0.1/32"]).is_err());
    assert!(
        MetricsAuthenticator::load(&config(workspace.token("short")?, &["127.0.0.1/32"],)?)
            .is_err()
    );
    assert!(
        MetricsAuthenticator::load(&config(
            workspace.token(&TOKEN.to_uppercase())?,
            &["127.0.0.1/32"],
        )?)
        .is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn credential_must_be_private_and_not_a_symlink() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let workspace = Workspace::new()?;
    let token = workspace.token(TOKEN)?;
    fs::set_permissions(&token, fs::Permissions::from_mode(0o644))?;
    assert!(MetricsAuthenticator::load(&config(token.clone(), &["127.0.0.1/32"])?).is_err());
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600))?;
    let link = workspace.0.join("metrics-link.secret");
    symlink(&token, &link)?;
    assert!(MetricsAuthenticator::load(&config(link, &["127.0.0.1/32"])?).is_err());
    Ok(())
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
