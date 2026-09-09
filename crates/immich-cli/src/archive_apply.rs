use immich_rs_application::{CancellationToken, apply_archive_manifest};

use crate::args::ArchiveApplyRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ArchiveApplyRequest) -> Result<(), CliFailure> {
    let manifest = output::load_archive_manifest(&request.manifest)?;
    let client = network::archive_client(
        &request.server,
        request.production_read,
        request.ca_certificate.as_deref(),
    )?;
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let report = apply_archive_manifest(&manifest, &request.destination, &client, &cancellation)
        .await
        .map_err(|error| CliFailure::from_application(&error))?;
    output::write_json(&report, "archive apply report")
}
