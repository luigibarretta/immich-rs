use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn private_ranges_allow_exact_policy_and_deny_mixed_rebinding()
-> Result<(), Box<dyn std::error::Error>> {
    let ranges = parse_ranges(&["fd42::/64".to_owned()])?;
    let allowed = vec!["[fd42::5]:443".parse()?, "[fd42::6]:443".parse()?];
    assert_eq!(
        validate_addresses(ServerMode::ProductionRead, &ranges, 4, allowed)?.len(),
        2
    );
    let mixed = vec!["[fd42::5]:443".parse()?, "192.0.2.8:443".parse()?];
    assert!(validate_addresses(ServerMode::ProductionRead, &ranges, 4, mixed).is_err());
    Ok(())
}

#[test]
fn zero_prefixes_match_their_address_family() -> Result<(), Box<dyn std::error::Error>> {
    assert!(AddressRange::parse("0.0.0.0/0")?.contains("203.0.113.8".parse()?));
    assert!(AddressRange::parse("::/0")?.contains("2001:db8::5".parse()?));
    assert!(!AddressRange::parse("192.0.2.0/24")?.contains("::1".parse()?));
    Ok(())
}

#[test]
fn read_client_uses_private_secret_and_pinned_loopback() -> Result<(), Box<dyn std::error::Error>> {
    reset_client_construction_count();
    crate::profiles::protected_file::reset_secret_read_count();
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "immich-rs-web-server-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&root)?;
    let secret = root.join("api-key.secret");
    fs::write(&secret, "synthetic-api-key\n")?;
    make_private(&secret)?;
    let profile = ServerProfile::from_raw(RawServerProfile {
        id: "disposable".to_owned(),
        origin: "http://127.0.0.1:2283".to_owned(),
        api_key_file: secret,
        ca_certificate_file: None,
        mode: ServerMode::Disposable,
        allowed_cidrs: Vec::new(),
        generation: 1,
        credential_generation: 1,
    })?;
    assert!(profile.read_client(4).is_ok());
    assert_eq!(client_construction_count(), 1);
    assert_eq!(crate::profiles::protected_file::secret_read_count(), 1);
    fs::remove_dir_all(root)?;
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
