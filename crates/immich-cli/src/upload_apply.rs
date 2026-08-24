use immich_rs_client::{ImmichReadClient, NegotiatedServer, upload_plan_sha256};
use immich_rs_core::{
    CancellationToken, ProductionWriteConfirmation, SourceKind, UPLOAD_PLAN_SCHEMA_VERSION,
    UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadPlan,
};
use immich_rs_executor::{
    ApplePhotosImportConfig, TakeoutImportConfig, apply_apple_photos_import,
    apply_production_apple_photos_import, apply_production_takeout_import, apply_production_upload,
    apply_takeout_import, apply_upload,
};

use crate::args::ApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ApplyRequest) -> Result<(), CliFailure> {
    let plan = output::load_upload_plan(&request.plan)?;
    if !matches!(
        plan.schema_version,
        UPLOAD_PLAN_SCHEMA_VERSION | UPLOAD_PLAN_SCHEMA_VERSION_V2
    ) {
        return Err(CliFailure::usage(
            "apply upload does not support this plan schema",
        ));
    }
    let production = production_confirmation(&request, &plan)?;
    let server = request
        .server
        .as_deref()
        .ok_or_else(|| CliFailure::usage("--server is required for apply"))?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let read_client = network::read_client(
        server,
        production.is_some(),
        request.ca_certificate.as_deref(),
    )?;
    let negotiated = read_client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    if negotiated.compatibility() != &plan.server {
        return Err(CliFailure::usage("server does not match upload plan"));
    }
    if plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2 {
        return run_import(
            request,
            &plan,
            read_client,
            negotiated,
            production,
            &cancellation,
        )
        .await;
    }
    let source = request
        .inputs
        .first()
        .filter(|_| request.inputs.len() == 1)
        .ok_or_else(|| CliFailure::usage("folder apply requires exactly one source"))?;
    let report = if let Some(confirmation) = production {
        let client = read_client
            .authorize_production_upload(negotiated, &plan, &confirmation)
            .map_err(CliFailure::from_client)?;
        apply_production_upload(
            &plan,
            source,
            &request.checkpoint,
            &request.config,
            &client,
            &cancellation,
        )
        .await
        .map_err(CliFailure::from_executor)?
    } else {
        let client = read_client
            .authorize_upload(negotiated)
            .map_err(CliFailure::from_client)?;
        apply_upload(
            &plan,
            source,
            &request.checkpoint,
            &request.config,
            &client,
            &cancellation,
        )
        .await
        .map_err(CliFailure::from_executor)?
    };
    if report.cancelled {
        return Err(CliFailure::cancelled());
    }
    output::write_json(&report, "apply report")
}

fn production_confirmation(
    request: &ApplyRequest,
    plan: &UploadPlan,
) -> Result<Option<ProductionWriteConfirmation>, CliFailure> {
    let confirmation = request
        .production
        .as_ref()
        .map(|production| {
            ProductionWriteConfirmation::new(
                true,
                production.plan_sha256.clone(),
                production.expected_operations,
                production.backup_reference.clone(),
            )
            .map_err(|_| CliFailure::usage("invalid production confirmation"))
        })
        .transpose()?;
    let Some(value) = &confirmation else {
        return Ok(None);
    };
    let digest = upload_plan_sha256(plan)
        .map_err(|_| CliFailure::usage("invalid production upload plan"))?;
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
        .ok_or_else(|| CliFailure::usage("production confirmation does not match upload plan"))
}

async fn run_import(
    request: ApplyRequest,
    plan: &UploadPlan,
    read_client: ImmichReadClient,
    negotiated: NegotiatedServer,
    production: Option<ProductionWriteConfirmation>,
    cancellation: &CancellationToken,
) -> Result<(), CliFailure> {
    let report = match plan.source.kind {
        SourceKind::GoogleTakeout => {
            let config = TakeoutImportConfig {
                source: request.takeout,
                upload: request.config,
            };
            if let Some(confirmation) = production {
                let client = read_client
                    .authorize_production_import(negotiated, plan, &confirmation)
                    .map_err(CliFailure::from_client)?;
                apply_production_takeout_import(
                    plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            } else {
                let client = read_client
                    .authorize_import(negotiated)
                    .map_err(CliFailure::from_client)?;
                apply_takeout_import(
                    plan,
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
            if let Some(confirmation) = production {
                let client = read_client
                    .authorize_production_import(negotiated, plan, &confirmation)
                    .map_err(CliFailure::from_client)?;
                apply_production_apple_photos_import(
                    plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            } else {
                let client = read_client
                    .authorize_import(negotiated)
                    .map_err(CliFailure::from_client)?;
                apply_apple_photos_import(
                    plan,
                    &request.inputs,
                    &request.checkpoint,
                    &config,
                    &client,
                    cancellation,
                )
                .await
            }
        }
        SourceKind::Folder => return Err(CliFailure::usage("unsupported import source kind")),
    }
    .map_err(CliFailure::from_executor)?;
    if report.cancelled {
        return Err(CliFailure::cancelled());
    }
    output::write_json(&report, "import apply report")
}
