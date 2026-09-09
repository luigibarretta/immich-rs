#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use immich_rs_web::WebConfig;

static WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-web-lan-config-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        fs::create_dir(root.join("source"))?;
        fs::create_dir(root.join("state"))?;
        make_private_directory(&root.join("state"))?;
        Ok(Self { root })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn write_config(&self, lan: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = self.path("web.toml");
        fs::write(&path, configuration(self, lan))?;
        Ok(path)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

fn configuration(workspace: &TestWorkspace, lan: &str) -> String {
    format!(
        r#"schema_version = 1

[web]
listen_address = "0.0.0.0:2285"
public_origin = "https://console.example:2285"
history_state_id = "console"
{lan}

[[sources]]
id = "camera_roll"
label = "Synthetic source"
allowed_root = "{}"
relative_root = "."
generation = 1

[[servers]]
id = "disposable"
origin = "http://127.0.0.1:9387"
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
        workspace.path("source").display(),
        workspace.path("never-read-api-key.secret").display(),
        workspace.root.display(),
    )
}

fn lan_policy(workspace: &TestWorkspace) -> String {
    format!(
        r#"[web.lan]
tls_certificate_file = "{}"
tls_private_key_file = "{}"

[web.lan.oidc]
issuer = "https://idp.internal.example/"
client_id = "immich-rs-web"
client_secret_file = "{}"
ca_certificate_file = "{}"
allowed_cidrs = ["192.0.2.0/24", "fd42::/64"]
allowed_subjects = ["operator-1"]
allowed_algorithm = "EdDSA"
"#,
        workspace.path("tls.crt").display(),
        workspace.path("tls.key").display(),
        workspace.path("oidc.secret").display(),
        workspace.path("idp-ca.crt").display(),
    )
}

#[test]
fn complete_lan_policy_allows_private_idp_ranges() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let path = workspace.write_config(&lan_policy(&workspace))?;
    let config = WebConfig::load(&path)?;
    assert_eq!(config.public_origin(), "https://console.example:2285");
    assert_eq!(config.allowed_host(), "console.example:2285");
    assert!(config.secure_cookies());
    assert!(config.bootstrap_secret_file().is_none());
    Ok(())
}

#[test]
fn lan_policy_rejects_incomplete_or_ambiguous_auth() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let valid = lan_policy(&workspace);
    let mut invalid_configs = vec![configuration(&workspace, &valid).replace(
        "https://console.example:2285",
        "http://console.example:2285",
    )];
    invalid_configs.extend(
        [
            valid.replace(
                "https://idp.internal.example/",
                "http://idp.internal.example/",
            ),
            valid.replace("[\"192.0.2.0/24\", \"fd42::/64\"]", "[]"),
            valid.replace(
                "allowed_subjects = [\"operator-1\"]",
                "allowed_subjects = [\"operator-1\"]\nrequired_role = \"operator\"",
            ),
            valid.replace(
                "allowed_algorithm = \"EdDSA\"",
                "allowed_algorithm = \"RS256\"",
            ),
        ]
        .map(|lan| configuration(&workspace, &lan)),
    );
    for invalid in invalid_configs {
        let path = workspace.path("web.toml");
        fs::write(&path, invalid)?;
        assert!(WebConfig::load(&path).is_err());
    }
    let ambiguous = format!(
        "bootstrap_secret_file = \"{}\"\n{}",
        workspace.path("bootstrap.secret").display(),
        valid
    );
    let path = workspace.write_config(&ambiguous)?;
    assert!(WebConfig::load(&path).is_err());
    Ok(())
}

#[test]
fn loopback_and_lan_auth_modes_are_mutually_exclusive() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let contents = configuration(&workspace, &lan_policy(&workspace))
        .replace("0.0.0.0:2285", "127.0.0.1:2285")
        .replace("https://console.example:2285", "http://127.0.0.1:2285");
    let path = workspace.path("web.toml");
    fs::write(&path, contents)?;
    assert!(WebConfig::load(&path).is_err());
    Ok(())
}

#[cfg(unix)]
fn make_private_directory(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(windows)]
fn make_private_directory(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
