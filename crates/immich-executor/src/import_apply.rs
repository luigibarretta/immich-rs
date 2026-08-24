use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use immich_rs_client::{ImmichImportClient, ProductionImmichImportClient};
use immich_rs_core::{
    Cancellation, CancellationToken, IMPORT_APPLY_REPORT_SCHEMA_VERSION, ImportApplyReport,
    UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadOperation, UploadPlan, UploadRole,
};
use immich_rs_sources::ResolvedFolderPlan;

use crate::import_config::ImportConfig;
use crate::import_effects::{apply_albums, apply_metadata};
use crate::import_journal::{ImportEvent, ImportJournal, ImportOutcome, ImportState, asset_key};
use crate::import_staging::ImportStaging;
use crate::journal::OutcomeKind;
use crate::operation::{OperationOutcome, OperationResult};
use crate::retry::RetryBudget;
use crate::verify::VerifiedAsset;
use crate::{ApplePhotosImportConfig, ExecutorError, ExecutorErrorClass, TakeoutImportConfig};

/// Apply one immutable Google Takeout plan to an authorized disposable Immich instance.
pub async fn apply_takeout_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &TakeoutImportConfig,
    client: &ImmichImportClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    apply_import_inner(plan, inputs, checkpoint, config, client, None, cancellation).await
}

/// Apply one exactly authorized production Takeout plan.
pub async fn apply_production_takeout_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &TakeoutImportConfig,
    client: &ProductionImmichImportClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    if !client.authorization().matches_plan(plan) || !client.import().upload().is_production() {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    apply_import_inner(
        plan,
        inputs,
        checkpoint,
        config,
        client.import(),
        Some(client.authorization().backup_reference_sha256()),
        cancellation,
    )
    .await
}

/// Apply one immutable Apple Photos plan to an authorized disposable Immich instance.
pub async fn apply_apple_photos_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &ApplePhotosImportConfig,
    client: &ImmichImportClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    apply_import_inner(plan, inputs, checkpoint, config, client, None, cancellation).await
}

/// Apply one exactly authorized production Apple Photos plan.
pub async fn apply_production_apple_photos_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &ApplePhotosImportConfig,
    client: &ProductionImmichImportClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    if !client.authorization().matches_plan(plan) || !client.import().upload().is_production() {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    apply_import_inner(
        plan,
        inputs,
        checkpoint,
        config,
        client.import(),
        Some(client.authorization().backup_reference_sha256()),
        cancellation,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_import_inner(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &impl ImportConfig,
    client: &ImmichImportClient,
    backup_reference_sha256: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    validate_context(plan, config, client, backup_reference_sha256.is_some())?;
    let resolved = rescan(plan, inputs, config, cancellation)?;
    let mut journal = match backup_reference_sha256 {
        Some(digest) => ImportJournal::open_production(checkpoint, plan, digest)?,
        None => ImportJournal::open(checkpoint, plan)?,
    };
    let state = journal.state()?;
    state.validate_for_plan(plan)?;
    let staging = ImportStaging::open(checkpoint, plan, config)?;
    let retry_budget = RetryBudget::new(config.upload().max_retries_per_run);
    let mut report = apply_report(plan);
    let mut asset_ids = initial_asset_ids(plan, &state);

    for operation in ordered_operations(plan)? {
        if cancellation.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let asset_id = if let Some(asset_id) = state.asset_id(&operation.operation_id) {
            report.resumed_effects = report.resumed_effects.saturating_add(1);
            asset_id.to_owned()
        } else {
            let live_video_id = live_video_id(operation, &asset_ids);
            if matches!(operation.role, UploadRole::LivePhotoImage { .. })
                && live_video_id.is_none()
            {
                append_asset_failure(&mut journal, operation, &mut report)?;
                continue;
            }
            let prepared = staging.prepare(&resolved, operation, cancellation)?;
            let source = VerifiedAsset {
                media_path: prepared.media_path.clone(),
                file_name: prepared.file_name.clone(),
                sha1_base64: prepared.sha1_base64.clone(),
                xmp: prepared.xmp.clone(),
            };
            let outcome = crate::operation::apply(
                operation,
                &source,
                live_video_id,
                client.upload(),
                config.upload(),
                &retry_budget,
                cancellation,
            )
            .await;
            drop(prepared);
            let Some(asset_id) = record_asset(&mut journal, operation, outcome, &mut report)?
            else {
                if report.cancelled {
                    break;
                }
                continue;
            };
            asset_ids.insert(operation.operation_id.clone(), asset_id.clone());
            asset_id
        };
        if !apply_metadata(
            operation,
            &asset_id,
            &state,
            &mut journal,
            &mut report,
            client,
            config.upload(),
            &retry_budget,
            cancellation,
        )
        .await?
        {
            break;
        }
    }
    if !report.cancelled && asset_ids.len() == plan.operations.len() {
        apply_albums(
            plan,
            &asset_ids,
            &state,
            &mut journal,
            &mut report,
            client,
            config.upload(),
            &retry_budget,
            cancellation,
        )
        .await?;
    }
    Ok(report)
}

fn validate_context(
    plan: &UploadPlan,
    config: &impl ImportConfig,
    client: &ImmichImportClient,
    production: bool,
) -> Result<(), ExecutorError> {
    config.validate_import()?;
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    let valid = plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2
        && plan.source.kind == config.source_kind()
        && plan.configuration_sha256 == config.identity()?
        && client.upload().compatibility() == &plan.server
        && client.upload().is_production() == production;
    valid
        .then_some(())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))
}

fn rescan(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    config: &impl ImportConfig,
    cancellation: &CancellationToken,
) -> Result<ResolvedFolderPlan, ExecutorError> {
    let resolved = config
        .scan_resolved(inputs, &plan.source.label, cancellation)
        .map_err(|error| match error {
            immich_rs_sources::ScanError::Cancelled => {
                ExecutorError::new(ExecutorErrorClass::Cancelled)
            }
            _ => ExecutorError::new(ExecutorErrorClass::SourceChanged),
        })?;
    let observed =
        crate::import_planner::create_import_upload_plan(&resolved, plan.server.clone(), config)?;
    if &observed != plan {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(resolved)
}

pub fn ordered_operations(plan: &UploadPlan) -> Result<Vec<&UploadOperation>, ExecutorError> {
    let mut groups = Vec::<Vec<&UploadOperation>>::new();
    for operation in &plan.operations {
        match &operation.role {
            UploadRole::Standalone => groups.push(vec![operation]),
            UploadRole::LivePhotoVideo { .. } => {
                let image = plan.operations.iter().find(|candidate| {
                    matches!(
                        &candidate.role,
                        UploadRole::LivePhotoImage { video_operation_id, .. }
                            if video_operation_id == &operation.operation_id
                    )
                });
                groups.push(vec![
                    operation,
                    image.ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?,
                ]);
            }
            UploadRole::LivePhotoImage { .. } => {}
        }
    }
    groups.sort_by(|left, right| left[0].relative_path.cmp(&right[0].relative_path));
    Ok(groups.into_iter().flatten().collect())
}

pub fn initial_asset_ids(plan: &UploadPlan, state: &ImportState) -> BTreeMap<String, String> {
    plan.operations
        .iter()
        .filter_map(|operation| {
            state
                .asset_id(&operation.operation_id)
                .map(|id| (operation.operation_id.clone(), id.to_owned()))
        })
        .collect()
}

pub fn live_video_id<'a>(
    operation: &UploadOperation,
    asset_ids: &'a BTreeMap<String, String>,
) -> Option<&'a str> {
    match &operation.role {
        UploadRole::LivePhotoImage {
            video_operation_id, ..
        } => asset_ids.get(video_operation_id).map(String::as_str),
        _ => None,
    }
}

pub fn record_asset(
    journal: &mut ImportJournal,
    operation: &UploadOperation,
    result: OperationResult,
    report: &mut ImportApplyReport,
) -> Result<Option<String>, ExecutorError> {
    match result {
        OperationResult::Completed(outcome) => record_completed_asset(journal, outcome, report),
        OperationResult::Cancelled(outcome) => {
            report.cancelled = true;
            if let Some(outcome) = outcome {
                journal.append(&ImportEvent {
                    effect_key: &asset_key(&operation.operation_id),
                    outcome: ImportOutcome::Indeterminate,
                    remote_id: None,
                    retries: outcome.retries,
                })?;
                report.retried = report.retried.saturating_add(u64::from(outcome.retries));
                report.indeterminate = report.indeterminate.saturating_add(1);
            }
            Ok(None)
        }
    }
}

fn record_completed_asset(
    journal: &mut ImportJournal,
    outcome: OperationOutcome,
    report: &mut ImportApplyReport,
) -> Result<Option<String>, ExecutorError> {
    let import_outcome = match outcome.kind {
        OutcomeKind::Created => ImportOutcome::Created,
        OutcomeKind::Duplicate => ImportOutcome::Duplicate,
        OutcomeKind::Failed => ImportOutcome::Failed,
        OutcomeKind::Indeterminate => ImportOutcome::Indeterminate,
    };
    journal.append(&ImportEvent {
        effect_key: &asset_key(&outcome.operation_id),
        outcome: import_outcome,
        remote_id: outcome.asset_id.as_deref(),
        retries: outcome.retries,
    })?;
    report.retried = report.retried.saturating_add(u64::from(outcome.retries));
    match outcome.kind {
        OutcomeKind::Created => report.created = report.created.saturating_add(1),
        OutcomeKind::Duplicate => report.duplicate = report.duplicate.saturating_add(1),
        OutcomeKind::Failed => report.failed = report.failed.saturating_add(1),
        OutcomeKind::Indeterminate => {
            report.indeterminate = report.indeterminate.saturating_add(1);
        }
    }
    Ok(outcome.asset_id)
}

pub fn append_asset_failure(
    journal: &mut ImportJournal,
    operation: &UploadOperation,
    report: &mut ImportApplyReport,
) -> Result<(), ExecutorError> {
    journal.append(&ImportEvent {
        effect_key: &asset_key(&operation.operation_id),
        outcome: ImportOutcome::Failed,
        remote_id: None,
        retries: 0,
    })?;
    report.failed = report.failed.saturating_add(1);
    Ok(())
}

pub fn apply_report(plan: &UploadPlan) -> ImportApplyReport {
    ImportApplyReport {
        schema_version: IMPORT_APPLY_REPORT_SCHEMA_VERSION,
        dry_run: false,
        planned: plan.summary,
        ..ImportApplyReport::default()
    }
}
