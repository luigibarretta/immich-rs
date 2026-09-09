use std::path::Path;

use immich_rs_client::ImmichReadClient;
use immich_rs_core::{CancellationToken, ImportApplyReport, MigrationPlan};
use immich_rs_executor::{
    MigrationApplyContext, MigrationPlanningConfig, apply_migration, create_migration_plan,
    dry_run_migration,
};

use crate::ApplicationError;

/// Probe two read capabilities and create one immutable migration plan.
pub async fn plan_migration(
    source: &ImmichReadClient,
    destination: &ImmichReadClient,
    config: MigrationPlanningConfig,
    cancellation: &CancellationToken,
) -> Result<MigrationPlan, ApplicationError> {
    let source_server = source.probe(cancellation).await?;
    let destination_server = destination.probe(cancellation).await?;
    Ok(create_migration_plan(
        source,
        &source_server,
        &destination_server,
        config,
        cancellation,
    )
    .await?)
}

/// Validate a migration plan offline without a checkpoint or client.
pub fn dry_run_migration_plan(
    plan: &MigrationPlan,
    config: &MigrationPlanningConfig,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ApplicationError> {
    Ok(dry_run_migration(plan, config, cancellation)?)
}

/// Probe, bind and apply a disposable two-server migration through the executor.
pub async fn apply_migration_plan(
    plan: &MigrationPlan,
    checkpoint: &Path,
    config: &MigrationPlanningConfig,
    source: &ImmichReadClient,
    destination: ImmichReadClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ApplicationError> {
    let source_server = source.probe(cancellation).await?;
    let destination_server = destination.probe(cancellation).await?;
    if source_server.migration_server() != plan.source_server
        || destination_server.migration_server() != plan.destination_server
    {
        return Err(ApplicationError::InvalidRequest(
            "migration servers do not match the immutable plan",
        ));
    }
    let destination_client = destination.authorize_import(destination_server.clone())?;
    let report = apply_migration(
        plan,
        checkpoint,
        config,
        MigrationApplyContext::new(
            source,
            &source_server,
            &destination_client,
            &destination_server,
        ),
        cancellation,
    )
    .await?;
    if report.cancelled {
        return Err(ApplicationError::Cancelled);
    }
    Ok(report)
}
