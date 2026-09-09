use immich_rs_application::{CancellationToken, plan_archive};

use crate::args::ArchivePlanRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ArchivePlanRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let client = network::archive_client(
        &request.server,
        request.production_read,
        request.ca_certificate.as_deref(),
    )?;
    let manifest = plan_archive(&request.config, &client, &cancellation)
        .await
        .map_err(|error| CliFailure::from_application(&error))?;
    output::write_json(&manifest, "archive manifest")
}
