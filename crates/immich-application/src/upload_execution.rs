use std::path::PathBuf;

use immich_rs_core::{
    ApplyReport, Cancellation, ImportApplyReport, SourceKind, UPLOAD_PLAN_SCHEMA_VERSION,
    UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadPlan,
};
use immich_rs_executor::{
    ApplePhotosImportConfig, PicasaImportConfig, TakeoutImportConfig, UploadExecutionConfig,
    dry_run_apple_photos_import, dry_run_picasa_import, dry_run_takeout_import, dry_run_upload,
};
use immich_rs_sources::{ApplePhotosScanConfig, PicasaScanConfig, TakeoutScanConfig};

use crate::ApplicationError;

/// Frontend-neutral inputs for validating an upload plan without remote effects.
#[derive(Clone, Debug)]
pub struct UploadDryRunRequest {
    /// Source roots or bounded split-archive inputs.
    pub inputs: Vec<PathBuf>,
    /// Durable checkpoint path used only for local validation.
    pub checkpoint: PathBuf,
    /// Folder upload execution bounds.
    pub config: UploadExecutionConfig,
    /// Google Takeout source options.
    pub takeout: TakeoutScanConfig,
    /// Apple Photos source options.
    pub apple: ApplePhotosScanConfig,
    /// Picasa source options.
    pub picasa: PicasaScanConfig,
}

/// Existing report shape selected by the immutable plan schema.
#[derive(Clone, Debug)]
pub enum UploadDryRunReport {
    /// Folder upload validation report.
    Folder(ApplyReport),
    /// Source-import validation report.
    Import(ImportApplyReport),
}

/// Validate one immutable upload plan using local inputs only.
pub fn dry_run_upload_plan(
    plan: &UploadPlan,
    request: &UploadDryRunRequest,
    cancellation: &impl Cancellation,
) -> Result<UploadDryRunReport, ApplicationError> {
    match plan.schema_version {
        UPLOAD_PLAN_SCHEMA_VERSION => dry_run_folder(plan, request, cancellation),
        UPLOAD_PLAN_SCHEMA_VERSION_V2 => dry_run_import(plan, request, cancellation),
        _ => Err(ApplicationError::InvalidRequest(
            "unsupported upload plan schema",
        )),
    }
}

fn dry_run_folder(
    plan: &UploadPlan,
    request: &UploadDryRunRequest,
    cancellation: &impl Cancellation,
) -> Result<UploadDryRunReport, ApplicationError> {
    let source = request
        .inputs
        .first()
        .filter(|_| request.inputs.len() == 1)
        .ok_or(ApplicationError::InvalidRequest(
            "folder dry-run requires exactly one source",
        ))?;
    let report = dry_run_upload(
        plan,
        source,
        &request.checkpoint,
        &request.config,
        cancellation,
    )?;
    Ok(UploadDryRunReport::Folder(report))
}

fn dry_run_import(
    plan: &UploadPlan,
    request: &UploadDryRunRequest,
    cancellation: &impl Cancellation,
) -> Result<UploadDryRunReport, ApplicationError> {
    let result = match plan.source.kind {
        SourceKind::GoogleTakeout => dry_run_takeout_import(
            plan,
            &request.inputs,
            &request.checkpoint,
            &TakeoutImportConfig {
                source: request.takeout.clone(),
                upload: request.config.clone(),
            },
            cancellation,
        ),
        SourceKind::ApplePhotos => dry_run_apple_photos_import(
            plan,
            &request.inputs,
            &request.checkpoint,
            &ApplePhotosImportConfig {
                source: request.apple.clone(),
                upload: request.config.clone(),
            },
            cancellation,
        ),
        SourceKind::Picasa => dry_run_picasa_import(
            plan,
            &request.inputs,
            &request.checkpoint,
            &PicasaImportConfig {
                source: request.picasa.clone(),
                upload: request.config.clone(),
            },
            cancellation,
        ),
        SourceKind::Folder => {
            return Err(ApplicationError::InvalidRequest(
                "unsupported import source kind",
            ));
        }
        SourceKind::Immich => {
            return Err(ApplicationError::InvalidRequest(
                "Immich migration plans require the migration command",
            ));
        }
    }?;
    Ok(UploadDryRunReport::Import(result))
}
