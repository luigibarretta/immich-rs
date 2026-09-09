use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::WebLimits;
use crate::auth::{AuthStore, PairingFailure};

const SECRET: &str = "synthetic-bootstrap-secret";
static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-web-auth-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn secret(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = self.root.join("bootstrap.secret");
        fs::write(&path, format!("{SECRET}\n"))?;
        make_private(&path)?;
        Ok(path)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
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

#[test]
fn pairing_limit_is_per_source_and_restart_forgets_sessions()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let secret_path = workspace.secret()?;
    let first = AuthStore::load(&secret_path, WebLimits::default())?;
    let pairing = first.pairing().ok_or("pairing unavailable")?;
    let ipv4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
    for _ in 0..5 {
        assert!(matches!(
            first.pair(ipv4, &pairing.cookie_token, &pairing.csrf_token, "wrong"),
            Err(PairingFailure::Denied)
        ));
    }
    assert!(matches!(
        first.pair(ipv4, &pairing.cookie_token, &pairing.csrf_token, SECRET),
        Err(PairingFailure::RateLimited)
    ));
    let established = first
        .pair(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            &pairing.cookie_token,
            &pairing.csrf_token,
            SECRET,
        )
        .map_err(|failure| format!("pairing failed: {failure:?}"))?;
    assert!(first.authenticate(&established.cookie_token).is_some());

    let restarted = AuthStore::load(&secret_path, WebLimits::default())?;
    assert!(restarted.authenticate(&established.cookie_token).is_none());
    assert!(restarted.pairing().is_some());
    Ok(())
}

#[test]
fn idle_session_expires_and_cannot_be_reused() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let secret_path = workspace.secret()?;
    let limits = WebLimits {
        session_idle_seconds: 1,
        ..WebLimits::default()
    };
    let store = AuthStore::load(&secret_path, limits)?;
    let pairing = store.pairing().ok_or("pairing unavailable")?;
    let established = store
        .pair(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            &pairing.cookie_token,
            &pairing.csrf_token,
            SECRET,
        )
        .map_err(|failure| format!("pairing failed: {failure:?}"))?;
    std::thread::sleep(Duration::from_millis(1_050));
    assert!(store.authenticate(&established.cookie_token).is_none());
    Ok(())
}

#[test]
fn oidc_session_rotation_expires_and_invalidates_previous_cookie()
-> Result<(), Box<dyn std::error::Error>> {
    let limits = WebLimits {
        session_idle_seconds: 1,
        ..WebLimits::default()
    };
    let store = AuthStore::oidc(limits);
    let first = store.establish_oidc("issuer\0operator-1", None)?;
    assert!(store.authenticate(&first.cookie_token).is_some());
    let second = store.establish_oidc("issuer\0operator-1", Some(&first.cookie_token))?;
    assert!(store.authenticate(&first.cookie_token).is_none());
    assert!(store.authenticate(&second.cookie_token).is_some());
    let restarted = AuthStore::oidc(limits);
    assert!(restarted.authenticate(&second.cookie_token).is_none());
    std::thread::sleep(Duration::from_millis(1_050));
    assert!(store.authenticate(&second.cookie_token).is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn bootstrap_secret_must_be_private_and_not_a_symlink() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let workspace = TestWorkspace::new()?;
    let secret_path = workspace.secret()?;
    fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o644))?;
    assert!(AuthStore::load(&secret_path, WebLimits::default()).is_err());

    fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o600))?;
    let link = workspace.root.join("bootstrap-link.secret");
    symlink(&secret_path, &link)?;
    assert!(AuthStore::load(&link, WebLimits::default()).is_err());
    Ok(())
}
