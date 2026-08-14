use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use immich_rs_client::ClientError;
use immich_rs_core::{Cancellation, CancellationToken};
use sha2::{Digest, Sha256};

use crate::UploadExecutionConfig;

#[derive(Clone, Debug)]
pub struct RetryBudget {
    used: Arc<AtomicU32>,
    maximum: u32,
}

impl RetryBudget {
    pub fn new(maximum: u32) -> Self {
        Self {
            used: Arc::new(AtomicU32::new(0)),
            maximum,
        }
    }

    pub fn claim(&self) -> bool {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < self.maximum).then_some(used.saturating_add(1))
            })
            .is_ok()
    }
}

pub fn delay(
    operation_id: &str,
    retry_count: u32,
    error: ClientError,
    config: &UploadExecutionConfig,
) -> Duration {
    if let Some(server_delay) = error.retry_after() {
        return server_delay.min(config.retry_delay_cap);
    }
    let exponent = retry_count.saturating_sub(1).min(20);
    let factor = 1_u32 << exponent;
    let exponential = config
        .retry_base_delay
        .saturating_mul(factor)
        .min(config.retry_delay_cap);
    let jitter_window = duration_millis(config.retry_base_delay) / 4;
    if jitter_window == 0 {
        return exponential;
    }
    let mut hasher = Sha256::new();
    hasher.update(operation_id.as_bytes());
    hasher.update(retry_count.to_be_bytes());
    let digest = hasher.finalize();
    let mut prefix = [0_u8; 8];
    prefix.copy_from_slice(&digest[..8]);
    let jitter = u64::from_be_bytes(prefix) % jitter_window;
    exponential
        .saturating_add(Duration::from_millis(jitter))
        .min(config.retry_delay_cap)
}

pub async fn wait(delay: Duration, cancellation: &CancellationToken) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => true,
        () = wait_cancelled(cancellation) => false,
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).map_or(u64::MAX, std::convert::identity)
}

async fn wait_cancelled(cancellation: &CancellationToken) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
