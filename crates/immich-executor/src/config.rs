use std::time::Duration;

use immich_rs_sources::FolderScanConfig;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{ExecutorError, ExecutorErrorClass};

/// Bounded scan, verification, concurrency and retry policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadExecutionConfig {
    /// Deterministic source rescan limits.
    pub scan: FolderScanConfig,
    /// Per-file SHA-256/SHA-1 verification buffer.
    pub verification_buffer_bytes: usize,
    /// Maximum concurrently owned operation groups.
    pub concurrency: usize,
    /// Maximum duplicate-check/upload attempts per operation.
    pub max_attempts_per_operation: u32,
    /// Maximum retries shared by the invocation.
    pub max_retries_per_run: u32,
    /// First deterministic backoff delay.
    pub retry_base_delay: Duration,
    /// Maximum deterministic or server-requested delay.
    pub retry_delay_cap: Duration,
}

impl Default for UploadExecutionConfig {
    fn default() -> Self {
        Self {
            scan: FolderScanConfig::default(),
            verification_buffer_bytes: 64 * 1_024,
            concurrency: 1,
            max_attempts_per_operation: 3,
            max_retries_per_run: 100,
            retry_base_delay: Duration::from_millis(100),
            retry_delay_cap: Duration::from_secs(10),
        }
    }
}

impl UploadExecutionConfig {
    pub(crate) fn validate(&self) -> Result<(), ExecutorError> {
        let valid = (4 * 1_024..=4 * 1_024 * 1_024).contains(&self.verification_buffer_bytes)
            && (1..=8).contains(&self.concurrency)
            && (1..=10).contains(&self.max_attempts_per_operation)
            && self.max_retries_per_run <= 10_000
            && !self.retry_base_delay.is_zero()
            && self.retry_base_delay <= self.retry_delay_cap
            && self.retry_delay_cap <= Duration::from_secs(300);
        valid
            .then_some(())
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))
    }

    pub(crate) fn identity_sha256(&self) -> Result<String, ExecutorError> {
        self.validate()?;
        let identity = ConfigurationIdentity {
            scan_buffer_bytes: self.scan.buffer_bytes,
            max_entries: self.scan.max_entries,
            max_directory_entries: self.scan.max_directory_entries,
            max_path_bytes: self.scan.max_path_bytes,
            case_sensitive: self.scan.case_sensitive,
            verification_buffer_bytes: self.verification_buffer_bytes,
            concurrency: self.concurrency,
            max_attempts_per_operation: self.max_attempts_per_operation,
            max_retries_per_run: self.max_retries_per_run,
            retry_base_delay_ms: duration_millis(self.retry_base_delay)?,
            retry_delay_cap_ms: duration_millis(self.retry_delay_cap)?,
        };
        let bytes = serde_json::to_vec(&identity)
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Serialize)]
struct ConfigurationIdentity {
    scan_buffer_bytes: usize,
    max_entries: usize,
    max_directory_entries: usize,
    max_path_bytes: usize,
    case_sensitive: bool,
    verification_buffer_bytes: usize,
    concurrency: usize,
    max_attempts_per_operation: u32,
    max_retries_per_run: u32,
    retry_base_delay_ms: u64,
    retry_delay_cap_ms: u64,
}

fn duration_millis(duration: Duration) -> Result<u64, ExecutorError> {
    u64::try_from(duration.as_millis())
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))
}
