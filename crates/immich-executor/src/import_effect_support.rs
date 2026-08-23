use std::collections::{BTreeMap, BTreeSet};

use immich_rs_client::{ClientError, ClientErrorClass};
use immich_rs_core::{CancellationToken, NormalizedMetadata, UploadPlan};

use crate::TakeoutImportConfig;
use crate::retry::{self, RetryBudget};

pub fn album_members(
    plan: &UploadPlan,
    asset_ids: &BTreeMap<String, String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut albums = BTreeMap::<String, BTreeSet<String>>::new();
    for operation in &plan.operations {
        let Some(asset_id) = asset_ids.get(&operation.operation_id) else {
            continue;
        };
        if let Some(metadata) = &operation.normalized_metadata {
            for album in &metadata.albums {
                albums
                    .entry(album.clone())
                    .or_default()
                    .insert(asset_id.clone());
            }
        }
    }
    albums
}

pub const fn has_assignment(metadata: &NormalizedMetadata) -> bool {
    metadata.description.is_some() || metadata.taken_at_utc.is_some() || metadata.location.is_some()
}

pub fn retry_allowed(
    error: ClientError,
    retries: u32,
    config: &TakeoutImportConfig,
    budget: &RetryBudget,
) -> bool {
    error.is_retryable()
        && retries.saturating_add(1) < config.upload.max_attempts_per_operation
        && budget.claim()
}

pub async fn wait_retry(
    key: &str,
    retries: u32,
    error: ClientError,
    config: &TakeoutImportConfig,
    cancellation: &CancellationToken,
) -> bool {
    let delay = retry::delay(key, retries.saturating_add(1), error, &config.upload);
    retry::wait(delay, cancellation).await
}

pub const fn uncertain(error: ClientError) -> bool {
    matches!(
        error.class(),
        ClientErrorClass::Disconnect | ClientErrorClass::Timeout
    )
}
