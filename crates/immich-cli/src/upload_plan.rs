use immich_rs_core::CancellationToken;
use immich_rs_executor::{UploadExecutionConfig, create_upload_plan};
use immich_rs_sources::{NoProgress, scan_folder_resolved};

use crate::args::UploadFolderRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: UploadFolderRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let client = network::read_client(
        &request.server,
        request.production_read,
        request.ca_certificate.as_deref(),
    )?;
    let negotiated = client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let resolved = scan_folder_resolved(
        &request.folder.root,
        &request.folder.label,
        &request.folder.config,
        &cancellation,
        &mut NoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    let mut config = UploadExecutionConfig::default();
    config.scan = request.folder.config;
    let plan = create_upload_plan(&resolved, negotiated.compatibility().clone(), &config)
        .map_err(CliFailure::from_executor)?;
    output::write_json(&plan, "upload plan")
}
