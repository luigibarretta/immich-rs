use std::collections::BTreeMap;
use std::path::Path;

use immich_rs_client::{
    ClientError, ClientErrorClass, ImmichImportClient, ImmichReadClient, NegotiatedServer,
};
use immich_rs_core::{
    Cancellation, CancellationToken, ImportApplyReport, MigrationPlan, UploadPlan, UploadRole,
};

use crate::import_apply::{
    append_asset_failure, apply_report, initial_asset_ids, live_video_id, ordered_operations,
    record_asset,
};
use crate::import_effects::{apply_albums, apply_metadata};
use crate::import_journal::ImportJournal;
use crate::migration_execution_plan::execution_plan;
use crate::migration_plan::{MigrationPlanningConfig, assemble_migration_plan};
use crate::migration_staging::MigrationStaging;
use crate::retry::RetryBudget;
use crate::{ExecutorError, ExecutorErrorClass};

/// Capabilities and probe proofs for both ends of one disposable migration.
pub struct MigrationApplyContext<'a> {
    source_client: &'a ImmichReadClient,
    source_server: &'a NegotiatedServer,
    destination_client: &'a ImmichImportClient,
    destination_server: &'a NegotiatedServer,
}

impl<'a> MigrationApplyContext<'a> {
    #[must_use]
    pub const fn new(
        source_client: &'a ImmichReadClient,
        source_server: &'a NegotiatedServer,
        destination_client: &'a ImmichImportClient,
        destination_server: &'a NegotiatedServer,
    ) -> Self {
        Self {
            source_client,
            source_server,
            destination_client,
            destination_server,
        }
    }
}

/// Validate one migration plan and emit its mutation ceiling without network access.
pub fn dry_run_migration(
    plan: &MigrationPlan,
    config: &MigrationPlanningConfig,
    cancellation: &impl Cancellation,
) -> Result<ImportApplyReport, ExecutorError> {
    if cancellation.is_cancelled() {
        return Err(ExecutorError::new(ExecutorErrorClass::Cancelled));
    }
    validate_plan_config(plan, config)?;
    let execution = execution_plan(plan)?;
    Ok(ImportApplyReport::dry_run(execution.summary))
}

/// Apply one immutable migration between two authorized disposable Immich instances.
pub async fn apply_migration(
    plan: &MigrationPlan,
    checkpoint: &Path,
    config: &MigrationPlanningConfig,
    context: MigrationApplyContext<'_>,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    let execution = prepare_execution(plan, config, &context, cancellation).await?;
    let mut journal = ImportJournal::open(checkpoint, &execution)?;
    let state = journal.state()?;
    state.validate_for_plan(&execution)?;
    let staging = MigrationStaging::open(checkpoint, plan, config.max_asset_bytes)?;
    let retry_budget = RetryBudget::new(config.upload.max_retries_per_run);
    let mut report = apply_report(&execution);
    let mut asset_ids = initial_asset_ids(&execution, &state);
    let source_assets = plan
        .assets
        .iter()
        .map(|asset| (asset.operation_id.as_str(), asset))
        .collect::<BTreeMap<_, _>>();

    for operation in ordered_operations(&execution)? {
        if cancellation.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let asset_id = if let Some(asset_id) = state.asset_id(&operation.operation_id) {
            report.resumed_effects = report.resumed_effects.saturating_add(1);
            asset_id.to_owned()
        } else {
            let live_video = live_video_id(operation, &asset_ids);
            if matches!(operation.role, UploadRole::LivePhotoImage { .. }) && live_video.is_none() {
                append_asset_failure(&mut journal, operation, &mut report)?;
                continue;
            }
            let source_asset = source_assets
                .get(operation.operation_id.as_str())
                .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
            let prepared = staging
                .prepare(
                    context.source_client,
                    context.source_server,
                    source_asset,
                    cancellation,
                )
                .await?;
            let verified = prepared.verified();
            let outcome = crate::operation::apply(
                operation,
                &verified,
                live_video,
                context.destination_client.upload(),
                &config.upload,
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
            context.destination_client,
            &config.upload,
            &retry_budget,
            cancellation,
        )
        .await?
        {
            break;
        }
    }
    if !report.cancelled && asset_ids.len() == execution.operations.len() {
        apply_albums(
            &execution,
            &asset_ids,
            &state,
            &mut journal,
            &mut report,
            context.destination_client,
            &config.upload,
            &retry_budget,
            cancellation,
        )
        .await?;
    }
    Ok(report)
}

async fn prepare_execution(
    plan: &MigrationPlan,
    config: &MigrationPlanningConfig,
    context: &MigrationApplyContext<'_>,
    cancellation: &CancellationToken,
) -> Result<UploadPlan, ExecutorError> {
    validate_context(
        plan,
        config,
        context.source_server,
        context.destination_client,
        context.destination_server,
    )?;
    verify_current_source(
        plan,
        config,
        context.source_client,
        context.source_server,
        cancellation,
    )
    .await
}

async fn verify_current_source(
    plan: &MigrationPlan,
    config: &MigrationPlanningConfig,
    source_client: &ImmichReadClient,
    source_server: &NegotiatedServer,
    cancellation: &CancellationToken,
) -> Result<UploadPlan, ExecutorError> {
    let inventory = source_client
        .migration_inventory(source_server, config.inventory, cancellation)
        .await
        .map_err(map_client_error)?;
    let hashes = plan
        .assets
        .iter()
        .map(|asset| (asset.source_asset_id.clone(), asset.content_sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    let observed = assemble_migration_plan(
        inventory,
        plan.source_server.clone(),
        plan.destination_server.clone(),
        &hashes,
        config.clone(),
    )?;
    if &observed != plan {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    execution_plan(plan)
}

fn validate_context(
    plan: &MigrationPlan,
    config: &MigrationPlanningConfig,
    source_server: &NegotiatedServer,
    destination: &ImmichImportClient,
    destination_server: &NegotiatedServer,
) -> Result<(), ExecutorError> {
    validate_plan_config(plan, config)?;
    let valid = source_server.migration_server() == plan.source_server
        && destination_server.migration_server() == plan.destination_server
        && destination.upload().compatibility() == &plan.destination_server.compatibility
        && !destination.upload().is_production();
    valid
        .then_some(())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))
}

fn validate_plan_config(
    plan: &MigrationPlan,
    config: &MigrationPlanningConfig,
) -> Result<(), ExecutorError> {
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    let validated = config.clone().validate()?;
    (validated.identity()? == plan.configuration_sha256)
        .then_some(())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))
}

fn map_client_error(error: ClientError) -> ExecutorError {
    if error.class() == ClientErrorClass::Cancelled {
        ExecutorError::new(ExecutorErrorClass::Cancelled)
    } else {
        ExecutorError::new(ExecutorErrorClass::Client)
    }
}
