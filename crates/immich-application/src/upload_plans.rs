use std::path::PathBuf;

use immich_rs_client::ImmichReadClient;
use immich_rs_core::{Cancellation, UploadPlan};
use immich_rs_executor::{
    ApplePhotosImportConfig, PicasaImportConfig, TakeoutImportConfig, UploadExecutionConfig,
    create_apple_photos_upload_plan, create_picasa_upload_plan, create_takeout_upload_plan,
    create_upload_plan,
};
use immich_rs_sources::{
    scan_apple_photos_inputs_resolved, scan_folder_resolved, scan_google_takeout_inputs_resolved,
    scan_picasa_inputs_resolved,
};

use crate::error::ApplicationError;
use crate::folder::FolderPlanRequest;
use crate::progress::{ApplicationProgressObserver, ScanProgressAdapter};

/// Inputs for the immutable folder upload-plan workflow.
#[derive(Clone, Debug)]
pub struct FolderUploadPlanRequest {
    /// Existing source-only folder request.
    pub source: FolderPlanRequest,
}

/// Inputs for one immutable source-aware upload-plan workflow.
#[derive(Clone, Debug)]
pub struct SourceUploadPlanRequest<Config> {
    /// One directory or bounded split-ZIP input set.
    pub inputs: Vec<PathBuf>,
    /// Source-neutral plan label.
    pub label: String,
    /// Adapter and upload planning configuration.
    pub config: Config,
}

/// Probe, scan and bind one folder plan in the established observable order.
pub async fn plan_folder_upload(
    request: &FolderUploadPlanRequest,
    client: &ImmichReadClient,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<UploadPlan, ApplicationError> {
    let negotiated = client.probe(cancellation).await?;
    let mut adapter = ScanProgressAdapter::new(observer);
    let resolved = scan_folder_resolved(
        &request.source.root,
        &request.source.label,
        &request.source.config,
        cancellation,
        &mut adapter,
    )?;
    let mut config = UploadExecutionConfig::default();
    config.scan = request.source.config.clone();
    Ok(create_upload_plan(
        &resolved,
        negotiated.compatibility().clone(),
        &config,
    )?)
}

/// Probe, scan and bind one Google Takeout upload plan.
pub async fn plan_google_takeout_upload(
    request: &SourceUploadPlanRequest<TakeoutImportConfig>,
    client: &ImmichReadClient,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<UploadPlan, ApplicationError> {
    let negotiated = client.probe(cancellation).await?;
    let mut adapter = ScanProgressAdapter::new(observer);
    let resolved = scan_google_takeout_inputs_resolved(
        &request.inputs,
        &request.label,
        &request.config.source,
        cancellation,
        &mut adapter,
    )?;
    Ok(create_takeout_upload_plan(
        &resolved,
        negotiated.compatibility().clone(),
        &request.config,
    )?)
}

/// Probe, scan and bind one Apple Photos upload plan.
pub async fn plan_apple_photos_upload(
    request: &SourceUploadPlanRequest<ApplePhotosImportConfig>,
    client: &ImmichReadClient,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<UploadPlan, ApplicationError> {
    let negotiated = client.probe(cancellation).await?;
    let mut adapter = ScanProgressAdapter::new(observer);
    let resolved = scan_apple_photos_inputs_resolved(
        &request.inputs,
        &request.label,
        &request.config.source,
        cancellation,
        &mut adapter,
    )?;
    Ok(create_apple_photos_upload_plan(
        &resolved,
        negotiated.compatibility().clone(),
        &request.config,
    )?)
}

/// Probe, scan and bind one Picasa upload plan.
pub async fn plan_picasa_upload(
    request: &SourceUploadPlanRequest<PicasaImportConfig>,
    client: &ImmichReadClient,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<UploadPlan, ApplicationError> {
    let negotiated = client.probe(cancellation).await?;
    let mut adapter = ScanProgressAdapter::new(observer);
    let resolved = scan_picasa_inputs_resolved(
        &request.inputs,
        &request.label,
        &request.config.source,
        cancellation,
        &mut adapter,
    )?;
    Ok(create_picasa_upload_plan(
        &resolved,
        negotiated.compatibility().clone(),
        &request.config,
    )?)
}
