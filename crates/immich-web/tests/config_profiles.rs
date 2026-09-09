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
            "immich-rs-web-config-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        Ok(Self { root })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

fn configuration(
    workspace: &TestWorkspace,
    source_root: &Path,
    extra_web: &str,
    relative_root: &str,
) -> String {
    let bootstrap = workspace.path("bootstrap.secret");
    let api_key = workspace.path("server-api-key.secret");
    format!(
        r#"schema_version = 1

[web]
listen_address = "127.0.0.1:2285"
public_origin = "http://127.0.0.1:2285"
bootstrap_secret_file = "{}"
{}

[[sources]]
id = "camera_roll"
label = "Synthetic camera roll"
allowed_root = "{}"
relative_root = "{}"
generation = 1

[sources.scan]
buffer_bytes = 4096
max_entries = 100
max_directory_entries = 50
max_path_bytes = 512
case_sensitive = true

[[servers]]
id = "disposable"
origin = "http://127.0.0.1:9387"
api_key_file = "{}"
generation = 1
"#,
        bootstrap.display(),
        extra_web,
        source_root.display(),
        relative_root,
        api_key.display(),
    )
}

fn write_config(
    workspace: &TestWorkspace,
    contents: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = workspace.path("web.toml");
    fs::write(&path, contents)?;
    Ok(path)
}

#[test]
fn strict_config_resolves_only_opaque_operator_profiles() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace = TestWorkspace::new()?;
    let allowed = workspace.path("allowed");
    let source = allowed.join("source");
    fs::create_dir_all(&source)?;
    fs::write(source.join("synthetic.jpg"), b"synthetic\n")?;
    let path = write_config(
        &workspace,
        &configuration(&workspace, &allowed, "", "source"),
    )?;

    let config = WebConfig::load(&path)?;
    assert_eq!(config.listen_address().to_string(), "127.0.0.1:2285");
    assert_eq!(config.public_origin(), "http://127.0.0.1:2285");
    assert_eq!(config.allowed_host(), "127.0.0.1:2285");
    assert_eq!(config.limits().request_header_bytes, 16 * 1024);
    assert_eq!(config.limits().request_body_bytes, 16 * 1024);
    assert_eq!(config.limits().accepted_connections, 16);
    assert_eq!(config.limits().header_read_seconds, 5);
    assert_eq!(config.limits().response_seconds, 15);
    assert_eq!(config.limits().concurrent_jobs, 1);
    assert_eq!(config.limits().queued_jobs, 4);
    assert_eq!(config.limits().retained_jobs, 128);
    assert_eq!(config.limits().sse_subscribers_per_session, 4);
    assert_eq!(config.limits().sse_subscribers_per_process, 16);
    assert_eq!(config.limits().sse_replay_events, 128);
    assert_eq!(config.limits().sse_heartbeat_seconds, 15);
    assert_eq!(config.sources().len(), 1);
    let profile = config
        .source("camera_roll")
        .ok_or("source profile missing")?;
    let resolved = profile.resolve()?;
    assert_eq!(resolved.root(), fs::canonicalize(&source)?);
    assert_eq!(resolved.label(), "Synthetic camera roll");
    assert_eq!(resolved.generation_sha256().len(), 64);
    assert_eq!(resolved.config().buffer_bytes, 4096);

    let server = config
        .server("disposable")
        .ok_or("server profile missing")?;
    assert_eq!(server.id(), "disposable");
    assert_eq!(server.origin(), "http://127.0.0.1:9387");
    assert!(!server.api_key_file().exists());
    assert_eq!(server.generation(), 1);
    Ok(())
}

#[test]
fn config_rejects_unknown_fields_traversal_and_excess_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let allowed = workspace.path("allowed");
    fs::create_dir(&allowed)?;

    let unknown = configuration(&workspace, &allowed, "unknown = true", ".");
    let unknown_path = write_config(&workspace, &unknown)?;
    assert!(WebConfig::load(&unknown_path).is_err());

    let traversal = configuration(&workspace, &allowed, "", "../outside");
    let traversal_path = write_config(&workspace, &traversal)?;
    assert!(WebConfig::load(&traversal_path).is_err());

    for limits in [
        "max_sessions = 17",
        "concurrent_jobs = 4\nqueued_jobs = 8\nretained_jobs = 11",
        "sse_subscribers_per_session = 8\nsse_subscribers_per_process = 7",
        "sse_replay_events = 257",
    ] {
        let excessive = configuration(
            &workspace,
            &allowed,
            &format!("\n[web.limits]\n{limits}"),
            ".",
        );
        let excessive_path = write_config(&workspace, &excessive)?;
        assert!(WebConfig::load(&excessive_path).is_err());
    }
    Ok(())
}

#[test]
fn source_identity_swap_is_rejected_before_scanning() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let allowed = workspace.path("allowed");
    let source = allowed.join("source");
    let parked = allowed.join("parked");
    fs::create_dir_all(&source)?;
    let path = write_config(
        &workspace,
        &configuration(&workspace, &allowed, "", "source"),
    )?;
    let config = WebConfig::load(&path)?;
    let profile = config
        .source("camera_roll")
        .ok_or("source profile missing")?;

    fs::rename(&source, &parked)?;
    fs::create_dir(&source)?;
    assert!(profile.resolve().is_err());
    Ok(())
}

#[test]
fn non_loopback_console_startup_is_refused_without_lan_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let allowed = workspace.path("allowed");
    fs::create_dir(&allowed)?;
    let contents =
        configuration(&workspace, &allowed, "", ".").replace("127.0.0.1:2285", "0.0.0.0:2285");
    let path = write_config(&workspace, &contents)?;
    assert!(WebConfig::load(&path).is_err());
    Ok(())
}
