use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use futures_util::stream::{self, StreamExt};
use immich_rs_client::{
    ClientError, ClientErrorClass, DuplicateCheck, ImmichUploadClient, UploadRequest, UploadResult,
};
use immich_rs_core::{
    ApplyReport, Cancellation, CancellationToken, UploadOperation, UploadPlan, UploadRole,
};

use crate::journal::{CompletedAsset, Journal, JournalEvent, OutcomeKind};
use crate::retry::{self, RetryBudget};
use crate::verify::{VerifiedAsset, verify_source};
use crate::{ExecutorError, ExecutorErrorClass, UploadExecutionConfig};

/// Validate source and an optional existing checkpoint without constructing an HTTP capability.
pub fn dry_run_upload(
    plan: &UploadPlan,
    root: &Path,
    checkpoint: &Path,
    config: &UploadExecutionConfig,
    cancellation: &impl Cancellation,
) -> Result<ApplyReport, ExecutorError> {
    config.validate()?;
    let _verified = verify_source(plan, root, config, cancellation)?;
    let completed = Journal::validate_existing(checkpoint, plan)?;
    validate_completed(plan, &completed)?;
    let mut report = ApplyReport::new(plan.summary.operations, true);
    report.resumed = completed.len() as u64;
    report.would_upload = report.planned.saturating_sub(report.resumed);
    Ok(report)
}

/// Apply one immutable upload plan with bounded structured concurrency and a durable journal.
pub async fn apply_upload(
    plan: &UploadPlan,
    root: &Path,
    checkpoint: &Path,
    config: &UploadExecutionConfig,
    client: &ImmichUploadClient,
    cancellation: &CancellationToken,
) -> Result<ApplyReport, ExecutorError> {
    config.validate()?;
    if client.compatibility() != &plan.server {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    let verified = verify_source(plan, root, config, cancellation)?;
    let mut journal = Journal::open(checkpoint, plan)?;
    let completed = journal.completed()?;
    validate_completed(plan, &completed)?;
    let groups = pending_groups(plan, &completed)?;
    let mut report = ApplyReport::new(plan.summary.operations, false);
    report.resumed = completed.len() as u64;
    let retry_budget = RetryBudget::new(config.max_retries_per_run);
    let mut results = stream::iter(groups)
        .map(|group| {
            apply_group(
                group,
                &verified,
                &completed,
                client,
                config,
                retry_budget.clone(),
                cancellation,
            )
        })
        .buffer_unordered(config.concurrency);
    while let Some(group_result) = results.next().await {
        if group_result.cancelled {
            report.cancelled = true;
        }
        for outcome in group_result.outcomes {
            journal.append(&JournalEvent {
                operation_id: &outcome.operation_id,
                kind: outcome.kind,
                asset_id: outcome.asset_id.as_deref(),
                retry_count: outcome.retries,
            })?;
            record_report(&mut report, &outcome);
        }
    }
    Ok(report)
}

struct OperationGroup<'a> {
    operations: Vec<&'a UploadOperation>,
}

struct GroupResult {
    outcomes: Vec<OperationOutcome>,
    cancelled: bool,
}

struct OperationOutcome {
    operation_id: String,
    kind: OutcomeKind,
    asset_id: Option<String>,
    retries: u32,
}

fn pending_groups<'a>(
    plan: &'a UploadPlan,
    completed: &BTreeMap<String, CompletedAsset>,
) -> Result<Vec<OperationGroup<'a>>, ExecutorError> {
    let by_id = plan
        .operations
        .iter()
        .map(|operation| (operation.operation_id.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let mut groups = Vec::new();
    for operation in &plan.operations {
        match &operation.role {
            UploadRole::Standalone if !completed.contains_key(&operation.operation_id) => {
                groups.push(OperationGroup {
                    operations: vec![operation],
                });
            }
            UploadRole::LivePhotoVideo { .. } => {
                let image = plan.operations.iter().find(|candidate| {
                    matches!(
                        &candidate.role,
                        UploadRole::LivePhotoImage { video_operation_id, .. }
                            if video_operation_id == &operation.operation_id
                    )
                });
                let image =
                    image.ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
                if !completed.contains_key(&operation.operation_id)
                    || !completed.contains_key(&image.operation_id)
                {
                    groups.push(OperationGroup {
                        operations: vec![operation, image],
                    });
                }
            }
            UploadRole::LivePhotoImage {
                video_operation_id, ..
            } if !by_id.contains_key(video_operation_id.as_str()) => {
                return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
            }
            _ => {}
        }
    }
    groups.sort_by(|left, right| {
        left.operations[0]
            .relative_path
            .cmp(&right.operations[0].relative_path)
    });
    Ok(groups)
}

async fn apply_group(
    group: OperationGroup<'_>,
    verified: &BTreeMap<String, VerifiedAsset>,
    completed: &BTreeMap<String, CompletedAsset>,
    client: &ImmichUploadClient,
    config: &UploadExecutionConfig,
    retry_budget: RetryBudget,
    cancellation: &CancellationToken,
) -> GroupResult {
    let mut outcomes = Vec::new();
    let mut video_asset_id = None;
    for operation in group.operations {
        if cancellation.is_cancelled() {
            return GroupResult {
                outcomes,
                cancelled: true,
            };
        }
        if let Some(done) = completed.get(&operation.operation_id) {
            if matches!(operation.role, UploadRole::LivePhotoVideo { .. }) {
                video_asset_id = Some(done.asset_id.clone());
            }
            continue;
        }
        if matches!(operation.role, UploadRole::LivePhotoImage { .. }) && video_asset_id.is_none() {
            outcomes.push(failed_outcome(operation, 0));
            continue;
        }
        let Some(source) = verified.get(&operation.operation_id) else {
            outcomes.push(failed_outcome(operation, 0));
            continue;
        };
        let outcome = apply_operation(
            operation,
            source,
            video_asset_id.as_deref(),
            client,
            config,
            &retry_budget,
            cancellation,
        )
        .await;
        if matches!(operation.role, UploadRole::LivePhotoVideo { .. }) {
            video_asset_id.clone_from(&outcome.asset_id);
        }
        let stop_dependency = outcome.asset_id.is_none()
            && matches!(operation.role, UploadRole::LivePhotoVideo { .. });
        outcomes.push(outcome);
        if stop_dependency {
            break;
        }
    }
    GroupResult {
        outcomes,
        cancelled: false,
    }
}

async fn apply_operation(
    operation: &UploadOperation,
    source: &VerifiedAsset,
    live_photo_video_id: Option<&str>,
    client: &ImmichUploadClient,
    config: &UploadExecutionConfig,
    retry_budget: &RetryBudget,
    cancellation: &CancellationToken,
) -> OperationOutcome {
    let mut retries = 0_u32;
    loop {
        let duplicate = client
            .duplicate_check(&operation.operation_id, &source.sha1_base64, cancellation)
            .await;
        match duplicate {
            Ok(DuplicateCheck::Duplicate(asset_id)) => {
                return successful_outcome(operation, OutcomeKind::Duplicate, asset_id, retries);
            }
            Ok(DuplicateCheck::Accept) => {}
            Err(error) => {
                if retry_allowed(error, retries, config, retry_budget)
                    && retry::wait(
                        retry::delay(&operation.operation_id, retries + 1, error, config),
                        cancellation,
                    )
                    .await
                {
                    retries += 1;
                    continue;
                }
                return failed_outcome(operation, retries);
            }
        }
        let request = UploadRequest {
            operation_id: &operation.operation_id,
            file_name: &source.file_name,
            media_path: &source.media_path,
            media_len: operation.byte_len,
            sha1_base64: &source.sha1_base64,
            created_at_unix_ms: operation.created_at_unix_ms,
            modified_at_unix_ms: operation.modified_at_unix_ms,
            xmp: source
                .xmp
                .as_ref()
                .map(|(path, len)| (path.as_path(), *len)),
            live_photo_video_id,
        };
        match client.upload(&request, cancellation).await {
            Ok((status, asset_id)) => {
                let kind = match status {
                    UploadResult::Created => OutcomeKind::Created,
                    UploadResult::Duplicate => OutcomeKind::Duplicate,
                };
                return successful_outcome(operation, kind, asset_id, retries);
            }
            Err(error) if retry_allowed(error, retries, config, retry_budget) => {
                if retry::wait(
                    retry::delay(&operation.operation_id, retries + 1, error, config),
                    cancellation,
                )
                .await
                {
                    retries += 1;
                    continue;
                }
                return indeterminate_outcome(operation, retries);
            }
            Err(error) if uncertain(error) => {
                return reconcile_uncertain(operation, source, client, cancellation, retries).await;
            }
            Err(_) => return failed_outcome(operation, retries),
        }
    }
}

async fn reconcile_uncertain(
    operation: &UploadOperation,
    source: &VerifiedAsset,
    client: &ImmichUploadClient,
    cancellation: &CancellationToken,
    retries: u32,
) -> OperationOutcome {
    match client
        .duplicate_check(&operation.operation_id, &source.sha1_base64, cancellation)
        .await
    {
        Ok(DuplicateCheck::Duplicate(asset_id)) => {
            successful_outcome(operation, OutcomeKind::Duplicate, asset_id, retries)
        }
        _ => indeterminate_outcome(operation, retries),
    }
}

fn retry_allowed(
    error: ClientError,
    retries: u32,
    config: &UploadExecutionConfig,
    budget: &RetryBudget,
) -> bool {
    error.is_retryable() && retries + 1 < config.max_attempts_per_operation && budget.claim()
}

const fn uncertain(error: ClientError) -> bool {
    matches!(
        error.class(),
        ClientErrorClass::Disconnect | ClientErrorClass::Timeout
    )
}

fn successful_outcome(
    operation: &UploadOperation,
    kind: OutcomeKind,
    asset_id: String,
    retries: u32,
) -> OperationOutcome {
    OperationOutcome {
        operation_id: operation.operation_id.clone(),
        kind,
        asset_id: Some(asset_id),
        retries,
    }
}

fn failed_outcome(operation: &UploadOperation, retries: u32) -> OperationOutcome {
    OperationOutcome {
        operation_id: operation.operation_id.clone(),
        kind: OutcomeKind::Failed,
        asset_id: None,
        retries,
    }
}

fn indeterminate_outcome(operation: &UploadOperation, retries: u32) -> OperationOutcome {
    OperationOutcome {
        operation_id: operation.operation_id.clone(),
        kind: OutcomeKind::Indeterminate,
        asset_id: None,
        retries,
    }
}

fn validate_completed(
    plan: &UploadPlan,
    completed: &BTreeMap<String, CompletedAsset>,
) -> Result<(), ExecutorError> {
    let operation_ids = plan
        .operations
        .iter()
        .map(|operation| operation.operation_id.as_str())
        .collect::<BTreeSet<_>>();
    if completed
        .keys()
        .any(|operation_id| !operation_ids.contains(operation_id.as_str()))
    {
        return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
    }
    Ok(())
}

fn record_report(report: &mut ApplyReport, outcome: &OperationOutcome) {
    report.retried = report.retried.saturating_add(u64::from(outcome.retries));
    match outcome.kind {
        OutcomeKind::Created => report.created = report.created.saturating_add(1),
        OutcomeKind::Duplicate => report.duplicate = report.duplicate.saturating_add(1),
        OutcomeKind::Failed => report.failed = report.failed.saturating_add(1),
        OutcomeKind::Indeterminate => {
            report.indeterminate = report.indeterminate.saturating_add(1);
        }
    }
}
