use std::path::{Path, PathBuf};

use immich_rs_client::{ImmichImportClient, ProductionImmichImportClient};
use immich_rs_core::{
    Cancellation, CancellationToken, ImportApplyReport, ServerCompatibility, UploadPlan,
};
use immich_rs_sources::ResolvedFolderPlan;

use crate::{ExecutorError, ExecutorErrorClass, PicasaImportConfig};

pub fn create_picasa_upload_plan(
    resolved: &ResolvedFolderPlan,
    server: ServerCompatibility,
    config: &PicasaImportConfig,
) -> Result<UploadPlan, ExecutorError> {
    crate::import_planner::create_picasa_upload_plan(resolved, server, config)
}

pub fn dry_run_picasa_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &PicasaImportConfig,
    cancellation: &impl Cancellation,
) -> Result<ImportApplyReport, ExecutorError> {
    crate::import_dry_run::dry_run_import(plan, inputs, checkpoint, config, cancellation)
}

pub async fn apply_picasa_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &PicasaImportConfig,
    client: &ImmichImportClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    crate::import_apply::apply_import_inner(
        plan,
        inputs,
        checkpoint,
        config,
        client,
        None,
        cancellation,
    )
    .await
}

pub async fn apply_production_picasa_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &PicasaImportConfig,
    client: &ProductionImmichImportClient,
    cancellation: &CancellationToken,
) -> Result<ImportApplyReport, ExecutorError> {
    if !client.authorization().matches_plan(plan) || !client.import().upload().is_production() {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    crate::import_apply::apply_import_inner(
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
