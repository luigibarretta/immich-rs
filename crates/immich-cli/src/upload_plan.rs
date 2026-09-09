use immich_rs_application::{
    ApplicationNoProgress, CancellationToken, FolderPlanRequest, FolderUploadPlanRequest,
    plan_folder_upload,
};

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
    let plan = plan_folder_upload(
        &FolderUploadPlanRequest {
            source: FolderPlanRequest {
                root: request.folder.root,
                label: request.folder.label,
                config: request.folder.config,
            },
        },
        &client,
        &cancellation,
        &mut ApplicationNoProgress,
    )
    .await
    .map_err(|error| CliFailure::from_application(&error))?;
    output::write_json(&plan, "upload plan")
}
