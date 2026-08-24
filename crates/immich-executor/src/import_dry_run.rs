use std::path::{Path, PathBuf};

use immich_rs_core::{Cancellation, ImportApplyReport, UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadPlan};
use immich_rs_sources::ScanError;

use crate::import_config::ImportConfig;
use crate::import_planner::create_import_upload_plan;
use crate::{ApplePhotosImportConfig, ExecutorError, ExecutorErrorClass, TakeoutImportConfig};

/// Verify a Takeout import plan without creating a checkpoint or HTTP capability.
pub fn dry_run_takeout_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &TakeoutImportConfig,
    cancellation: &impl Cancellation,
) -> Result<ImportApplyReport, ExecutorError> {
    dry_run_import(plan, inputs, checkpoint, config, cancellation)
}

/// Verify an Apple Photos import plan without creating a checkpoint or HTTP capability.
pub fn dry_run_apple_photos_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &ApplePhotosImportConfig,
    cancellation: &impl Cancellation,
) -> Result<ImportApplyReport, ExecutorError> {
    dry_run_import(plan, inputs, checkpoint, config, cancellation)
}

fn dry_run_import(
    plan: &UploadPlan,
    inputs: &[PathBuf],
    checkpoint: &Path,
    config: &impl ImportConfig,
    cancellation: &impl Cancellation,
) -> Result<ImportApplyReport, ExecutorError> {
    config.validate_import()?;
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    if plan.schema_version != UPLOAD_PLAN_SCHEMA_VERSION_V2
        || plan.source.kind != config.source_kind()
        || plan.configuration_sha256 != config.identity()?
    {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidPlan));
    }
    if checkpoint_exists(checkpoint)? {
        return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
    }
    let resolved = config
        .scan_resolved(inputs, &plan.source.label, cancellation)
        .map_err(|error| scan_error(&error))?;
    let observed = create_import_upload_plan(&resolved, plan.server.clone(), config)?;
    if &observed != plan {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(ImportApplyReport::dry_run(plan.summary))
}

fn checkpoint_exists(path: &Path) -> Result<bool, ExecutorError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ExecutorError::new(ExecutorErrorClass::Checkpoint)),
    }
}

const fn scan_error(error: &ScanError) -> ExecutorError {
    match error {
        ScanError::Cancelled => ExecutorError::new(ExecutorErrorClass::Cancelled),
        _ => ExecutorError::new(ExecutorErrorClass::SourceChanged),
    }
}
