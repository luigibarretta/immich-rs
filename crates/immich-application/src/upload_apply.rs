use std::path::PathBuf;

use immich_rs_client::{ImmichReadClient, NegotiatedServer, upload_plan_sha256};
use immich_rs_core::{
    ApplyReport, CancellationToken, ImportApplyReport, ProductionWriteConfirmation, SourceKind,
    UPLOAD_PLAN_SCHEMA_VERSION, UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadPlan,
};
use immich_rs_executor::{
    ApplePhotosImportConfig, PicasaImportConfig, TakeoutImportConfig, UploadExecutionConfig,
    apply_apple_photos_import, apply_picasa_import, apply_production_apple_photos_import,
    apply_production_picasa_import, apply_production_takeout_import, apply_production_upload,
    apply_takeout_import, apply_upload,
};
use immich_rs_sources::{ApplePhotosScanConfig, PicasaScanConfig, TakeoutScanConfig};

use crate::ApplicationError;

/// Explicit operator values for the existing CLI production-write path.
pub struct ProductionWriteRequest {
    /// Canonical digest shown during immutable plan inspection.
    pub plan_sha256: String,
    /// Exact logical-effect ceiling shown during plan inspection.
    pub expected_operations: u64,
    /// Bounded operator reference retained only for authorization.
    pub backup_reference: String,
}

/// Validated apply admission that cannot be reconstructed from plan summaries.
pub struct PreparedUploadApply {
    plan: UploadPlan,
    production: Option<ProductionWriteConfirmation>,
}

impl PreparedUploadApply {
    /// Whether frontend policy must construct a production-capable read client.
    #[must_use]
    pub const fn is_production(&self) -> bool {
        self.production.is_some()
    }
}

/// Source inputs and execution limits for one admitted upload apply.
pub struct UploadApplyRequest {
    /// Source roots or bounded split-archive inputs.
    pub inputs: Vec<PathBuf>,
    /// Existing executor checkpoint path.
    pub checkpoint: PathBuf,
    /// Upload execution bounds.
    pub config: UploadExecutionConfig,
    /// Google Takeout source options.
    pub takeout: TakeoutScanConfig,
    /// Apple Photos source options.
    pub apple: ApplePhotosScanConfig,
    /// Picasa source options.
    pub picasa: PicasaScanConfig,
}

/// Existing report shape selected by the immutable plan schema.
pub enum UploadApplyReport {
    /// Folder upload report.
    Folder(ApplyReport),
    /// Source-import report.
    Import(ImportApplyReport),
}

/// Validate schema and optional production confirmation before any secret loading.
pub fn prepare_upload_apply(
    plan: UploadPlan,
    production: Option<ProductionWriteRequest>,
) -> Result<PreparedUploadApply, ApplicationError> {
    if !matches!(
        plan.schema_version,
        UPLOAD_PLAN_SCHEMA_VERSION | UPLOAD_PLAN_SCHEMA_VERSION_V2
    ) {
        return Err(ApplicationError::InvalidRequest(
            "apply upload does not support this plan schema",
        ));
    }
    let production = validate_production_confirmation(production, &plan)?;
    Ok(PreparedUploadApply { plan, production })
}

/// Probe, bind and execute one prepared apply through the sole executor owner.
pub async fn apply_prepared_upload(
    prepared: PreparedUploadApply,
    request: UploadApplyRequest,
    read_client: ImmichReadClient,
    cancellation: &CancellationToken,
) -> Result<UploadApplyReport, ApplicationError> {
    let negotiated = read_client.probe(cancellation).await?;
    if negotiated.compatibility() != &prepared.plan.server {
        return Err(ApplicationError::InvalidRequest(
            "server does not match upload plan",
        ));
    }
    if prepared.plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2 {
        apply_import(prepared, request, read_client, negotiated, cancellation).await
    } else {
        apply_folder(prepared, request, read_client, negotiated, cancellation).await
    }
}

fn validate_production_confirmation(
    request: Option<ProductionWriteRequest>,
    plan: &UploadPlan,
) -> Result<Option<ProductionWriteConfirmation>, ApplicationError> {
    let confirmation = request
        .map(|request| {
            ProductionWriteConfirmation::new(
                true,
                request.plan_sha256,
                request.expected_operations,
                request.backup_reference,
            )
            .map_err(|_| ApplicationError::InvalidRequest("invalid production confirmation"))
        })
        .transpose()?;
    let Some(value) = &confirmation else {
        return Ok(None);
    };
    let digest = upload_plan_sha256(plan)
        .map_err(|_| ApplicationError::InvalidRequest("invalid production upload plan"))?;
    let expected = if plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2 {
        plan.summary.max_mutations
    } else {
        plan.summary.operations
    };
    let matches = value.plan_sha256() == digest
        && value.expected_operations() == expected
        && (plan.schema_version != UPLOAD_PLAN_SCHEMA_VERSION
            || value.expected_operations() == plan.operations.len() as u64);
    matches
        .then_some(confirmation)
        .ok_or(ApplicationError::InvalidRequest(
            "production confirmation does not match upload plan",
        ))
}

async fn apply_folder(
    prepared: PreparedUploadApply,
    request: UploadApplyRequest,
    read_client: ImmichReadClient,
    negotiated: NegotiatedServer,
    cancellation: &CancellationToken,
) -> Result<UploadApplyReport, ApplicationError> {
    let source = request
        .inputs
        .first()
        .filter(|_| request.inputs.len() == 1)
        .ok_or(ApplicationError::InvalidRequest(
            "folder apply requires exactly one source",
        ))?;
    let report = if let Some(confirmation) = prepared.production {
        let client =
            read_client.authorize_production_upload(negotiated, &prepared.plan, &confirmation)?;
        apply_production_upload(
            &prepared.plan,
            source,
            &request.checkpoint,
            &request.config,
            &client,
            cancellation,
        )
        .await?
    } else {
        let client = read_client.authorize_upload(negotiated)?;
        apply_upload(
            &prepared.plan,
            source,
            &request.checkpoint,
            &request.config,
            &client,
            cancellation,
        )
        .await?
    };
    if report.cancelled {
        return Err(ApplicationError::Cancelled);
    }
    Ok(UploadApplyReport::Folder(report))
}

async fn apply_import(
    prepared: PreparedUploadApply,
    request: UploadApplyRequest,
    read_client: ImmichReadClient,
    negotiated: NegotiatedServer,
    cancellation: &CancellationToken,
) -> Result<UploadApplyReport, ApplicationError> {
    let report = match prepared.plan.source.kind {
        SourceKind::GoogleTakeout => {
            let config = TakeoutImportConfig {
                source: request.takeout,
                upload: request.config,
            };
            if let Some(confirmation) = prepared.production {
                let client = read_client.authorize_production_import(
                    negotiated,
                    &prepared.plan,
                    &confirmation,
                )?;
                apply_production_takeout_import(
                    &prepared.plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            } else {
                let client = read_client.authorize_import(negotiated)?;
                apply_takeout_import(
                    &prepared.plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            }
        }
        SourceKind::ApplePhotos => {
            let config = ApplePhotosImportConfig {
                source: request.apple,
                upload: request.config,
            };
            if let Some(confirmation) = prepared.production {
                let client = read_client.authorize_production_import(
                    negotiated,
                    &prepared.plan,
                    &confirmation,
                )?;
                apply_production_apple_photos_import(
                    &prepared.plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            } else {
                let client = read_client.authorize_import(negotiated)?;
                apply_apple_photos_import(
                    &prepared.plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            }
        }
        SourceKind::Picasa => {
            return apply_picasa(prepared, request, read_client, negotiated, cancellation).await;
        }
        SourceKind::Immich => {
            return Err(ApplicationError::InvalidRequest(
                "Immich migration plans require the migration command",
            ));
        }
        SourceKind::Folder => {
            return Err(ApplicationError::InvalidRequest(
                "unsupported import source kind",
            ));
        }
    }?;
    finish_import(report)
}

async fn apply_picasa(
    prepared: PreparedUploadApply,
    request: UploadApplyRequest,
    read_client: ImmichReadClient,
    negotiated: NegotiatedServer,
    cancellation: &CancellationToken,
) -> Result<UploadApplyReport, ApplicationError> {
    let config = PicasaImportConfig {
        source: request.picasa,
        upload: request.config,
    };
    let report = if let Some(confirmation) = prepared.production {
        let client =
            read_client.authorize_production_import(negotiated, &prepared.plan, &confirmation)?;
        apply_production_picasa_import(
            &prepared.plan,
            &request.inputs,
            &request.checkpoint,
            &config,
            &client,
            cancellation,
        )
        .await
    } else {
        let client = read_client.authorize_import(negotiated)?;
        apply_picasa_import(
            &prepared.plan,
            &request.inputs,
            &request.checkpoint,
            &config,
            &client,
            cancellation,
        )
        .await
    }?;
    finish_import(report)
}

const fn finish_import(report: ImportApplyReport) -> Result<UploadApplyReport, ApplicationError> {
    if report.cancelled {
        return Err(ApplicationError::Cancelled);
    }
    Ok(UploadApplyReport::Import(report))
}
