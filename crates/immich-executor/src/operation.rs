use immich_rs_client::{
    ClientError, ClientErrorClass, DuplicateCheck, ImmichUploadClient, UploadRequest, UploadResult,
};
use immich_rs_core::{Cancellation, CancellationToken, UploadOperation};

use crate::UploadExecutionConfig;
use crate::journal::OutcomeKind;
use crate::retry::{self, RetryBudget};
use crate::verify::VerifiedAsset;

pub struct OperationOutcome {
    pub operation_id: String,
    pub kind: OutcomeKind,
    pub asset_id: Option<String>,
    pub retries: u32,
}

pub enum OperationResult {
    Completed(OperationOutcome),
    Cancelled(Option<OperationOutcome>),
}

pub async fn apply(
    operation: &UploadOperation,
    source: &VerifiedAsset,
    live_photo_video_id: Option<&str>,
    client: &ImmichUploadClient,
    config: &UploadExecutionConfig,
    retry_budget: &RetryBudget,
    cancellation: &CancellationToken,
) -> OperationResult {
    let mut retries = 0_u32;
    loop {
        let duplicate = client
            .duplicate_check(&operation.operation_id, &source.sha1_base64, cancellation)
            .await;
        match duplicate {
            Ok(DuplicateCheck::Duplicate(asset_id)) => {
                return completed(successful(
                    operation,
                    OutcomeKind::Duplicate,
                    asset_id,
                    retries,
                ));
            }
            Ok(DuplicateCheck::Accept) => {}
            Err(error) if error.class() == ClientErrorClass::Cancelled => {
                return OperationResult::Cancelled(None);
            }
            Err(error) => {
                if retry_allowed(error, retries, config, retry_budget) {
                    let delay = retry::delay(&operation.operation_id, retries + 1, error, config);
                    if !retry::wait(delay, cancellation).await {
                        return OperationResult::Cancelled(None);
                    }
                    retries += 1;
                    continue;
                }
                return completed(failed(operation, retries));
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
                return completed(successful(operation, kind, asset_id, retries));
            }
            Err(error) if error.class() == ClientErrorClass::Cancelled => {
                return OperationResult::Cancelled(Some(indeterminate(operation, retries)));
            }
            Err(error) if retry_allowed(error, retries, config, retry_budget) => {
                let delay = retry::delay(&operation.operation_id, retries + 1, error, config);
                if !retry::wait(delay, cancellation).await {
                    return OperationResult::Cancelled(Some(indeterminate(operation, retries)));
                }
                retries += 1;
            }
            Err(error) if uncertain(error) => {
                return reconcile_uncertain(operation, source, client, cancellation, retries).await;
            }
            Err(_) => return completed(failed(operation, retries)),
        }
    }
}

async fn reconcile_uncertain(
    operation: &UploadOperation,
    source: &VerifiedAsset,
    client: &ImmichUploadClient,
    cancellation: &CancellationToken,
    retries: u32,
) -> OperationResult {
    let check = client
        .duplicate_check(&operation.operation_id, &source.sha1_base64, cancellation)
        .await;
    match check {
        Ok(DuplicateCheck::Duplicate(asset_id)) => completed(successful(
            operation,
            OutcomeKind::Duplicate,
            asset_id,
            retries,
        )),
        _ if cancellation.is_cancelled() => {
            OperationResult::Cancelled(Some(indeterminate(operation, retries)))
        }
        _ => completed(indeterminate(operation, retries)),
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

fn successful(
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

pub fn failed(operation: &UploadOperation, retries: u32) -> OperationOutcome {
    OperationOutcome {
        operation_id: operation.operation_id.clone(),
        kind: OutcomeKind::Failed,
        asset_id: None,
        retries,
    }
}

fn indeterminate(operation: &UploadOperation, retries: u32) -> OperationOutcome {
    OperationOutcome {
        operation_id: operation.operation_id.clone(),
        kind: OutcomeKind::Indeterminate,
        asset_id: None,
        retries,
    }
}

const fn completed(outcome: OperationOutcome) -> OperationResult {
    OperationResult::Completed(outcome)
}
