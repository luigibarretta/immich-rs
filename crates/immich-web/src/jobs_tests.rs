use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::WebConfig;
use crate::jobs::{AdmissionError, JobManager, JobStatus, SubscribeError, SubscriptionDelivery};
use crate::state_store::ConsoleStore;

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
        fs::create_dir(root.join("state"))?;
        make_private_directory(&root.join("state"))?;
        File::create(source.join("bounded.jpg"))?.set_len(512 * 1_024 * 1_024)?;
        let config = format!(
            r#"schema_version = 1

[web]
listen_address = "127.0.0.1:2285"
public_origin = "http://127.0.0.1:2285"
bootstrap_secret_file = "{}"
history_state_id = "console"

[web.limits]
concurrent_jobs = 1
queued_jobs = 1
retained_jobs = 2
sse_subscribers_per_session = 1
sse_subscribers_per_process = 1
sse_replay_events = 1

[[sources]]
id = "bounded"
label = "Bounded synthetic source"
allowed_root = "{}"
relative_root = "."
generation = 1

[[states]]
id = "console"
label = "Synthetic console state"
allowed_root = "{}"
relative_root = "state"
generation = 1
"#,
            root.join("unused.secret").display(),
            source.display(),
            root.display(),
        );
        fs::write(root.join("web.toml"), config)?;
        Ok(Self { root })
    }

    fn config(&self) -> Result<WebConfig, Box<dyn std::error::Error>> {
        Ok(WebConfig::load(&self.root.join("web.toml"))?)
    }

    fn manager(&self) -> Result<JobManager, Box<dyn std::error::Error>> {
        let config = Arc::new(self.config()?);
        let state_profile = config.history_state()?.resolve()?;
        let store = Arc::new(ConsoleStore::open(&state_profile, config.limits())?);
        Ok(JobManager::new(config, store))
    }
}

#[cfg(unix)]
fn make_private_directory(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(windows)]
fn make_private_directory(_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[tokio::test]
async fn subscriptions_bound_owners_replay_and_slow_consumers()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let manager = workspace.manager()?;
    let owner = [11_u8; 32];
    let other = [12_u8; 32];
    let running = manager
        .admit(owner, "bounded")
        .map_err(|_| "first job admission failed")?;
    let queued = manager
        .admit(other, "bounded")
        .map_err(|_| "second job admission failed")?;

    for _attempt in 0..100 {
        let status = manager
            .snapshot(&running, &owner)
            .ok_or("running job missing")?
            .status;
        if status == JobStatus::Running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(
        manager
            .snapshot(&running, &owner)
            .ok_or("running job missing")?
            .status,
        JobStatus::Running
    );
    assert!(matches!(
        manager.subscribe(&running, owner, Some(0)),
        Err(SubscribeError::ReplayExhausted)
    ));
    assert!(matches!(
        manager.subscribe(&running, owner, Some(u64::MAX)),
        Err(SubscribeError::InvalidCursor)
    ));
    let subscriber = manager
        .subscribe(&running, owner, None)
        .map_err(|_| "first subscription failed")?;
    assert!(matches!(
        manager.subscribe(&running, other, None),
        Err(SubscribeError::NotFound)
    ));
    assert!(matches!(
        manager.subscribe(&running, owner, None),
        Err(SubscribeError::SessionCapacity)
    ));
    assert!(matches!(
        manager.subscribe(&queued, other, None),
        Err(SubscribeError::ProcessCapacity)
    ));
    drop(subscriber);
    assert_eq!(
        manager
            .snapshot(&running, &owner)
            .ok_or("running job missing after disconnect")?
            .status,
        JobStatus::Running
    );
    let replacement = manager
        .subscribe(&queued, other, None)
        .map_err(|_| "subscriber lease was not released")?;
    drop(replacement);

    let mut slow = manager
        .subscribe(&running, owner, None)
        .map_err(|_| "slow subscription failed")?;
    assert!(manager.cancel(&running, &owner).is_some());
    assert!(manager.shutdown());
    assert!(matches!(
        slow.next(std::time::Duration::from_millis(1)).await,
        Some(SubscriptionDelivery::Event(_))
    ));
    assert_eq!(
        slow.next(std::time::Duration::from_millis(1)).await,
        Some(SubscriptionDelivery::ReplayExhausted)
    );
    let cancelled = manager
        .snapshot(&running, &owner)
        .ok_or("cancelled job missing")?;
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert!(cancelled.summary.is_none());
    Ok(())
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn jobs_are_bounded_owned_and_cancel_idempotently() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = Workspace::new()?;
    let manager = workspace.manager()?;
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
