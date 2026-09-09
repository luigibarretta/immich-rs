use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::WebConfig;
use crate::jobs::{AdmissionError, JobManager, JobStatus};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "immich-rs-web-jobs-{}-{sequence}",
            std::process::id()
        ));
        let source = root.join("source");
        fs::create_dir_all(&source)?;
        File::create(source.join("bounded.jpg"))?.set_len(512 * 1_024 * 1_024)?;
        let config = format!(
            r#"schema_version = 1

[web]
listen_address = "127.0.0.1:2285"
public_origin = "http://127.0.0.1:2285"
bootstrap_secret_file = "{}"

[web.limits]
concurrent_jobs = 1
queued_jobs = 1
retained_jobs = 2

[[sources]]
id = "bounded"
label = "Bounded synthetic source"
allowed_root = "{}"
relative_root = "."
generation = 1
"#,
            root.join("unused.secret").display(),
            source.display(),
        );
        fs::write(root.join("web.toml"), config)?;
        Ok(Self { root })
    }

    fn config(&self) -> Result<WebConfig, Box<dyn std::error::Error>> {
        Ok(WebConfig::load(&self.root.join("web.toml"))?)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn jobs_are_bounded_owned_and_cancel_idempotently() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let manager = JobManager::new(Arc::new(workspace.config()?));
    let owner = [7_u8; 32];
    let other = [8_u8; 32];

    assert_eq!(
        manager.admit(owner, "missing"),
        Err(AdmissionError::UnknownProfile)
    );
    let running = manager
        .admit(owner, "bounded")
        .map_err(|_| "first job admission failed")?;
    let queued = manager
        .admit(owner, "bounded")
        .map_err(|_| "second job admission failed")?;
    assert_eq!(manager.admit(owner, "bounded"), Err(AdmissionError::Full));
    assert!(manager.snapshot(&queued, &other).is_none());

    let first_cancel = manager
        .cancel(&queued, &owner)
        .ok_or("queued job missing")?;
    let second_cancel = manager
        .cancel(&queued, &owner)
        .ok_or("queued job missing")?;
    assert_eq!(first_cancel.status, JobStatus::Cancelled);
    assert_eq!(second_cancel.status, JobStatus::Cancelled);
    assert_eq!(first_cancel.progress, second_cancel.progress);
    assert!(second_cancel.summary.is_none());

    assert!(manager.cancel(&running, &owner).is_some());
    assert_eq!(manager.owned_worker_count(), Some(2));
    assert!(manager.shutdown());
    assert_eq!(manager.owned_worker_count(), Some(0));
    Ok(())
}
