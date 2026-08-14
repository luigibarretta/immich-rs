use immich_rs_core::CancellationToken;
use immich_rs_sources::{NoProgress, scan_folder};

use crate::args::FolderRequest;
use crate::failure::CliFailure;
use crate::{output, signal};

pub fn run(request: &FolderRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let plan = scan_folder(
        &request.root,
        &request.label,
        &request.config,
        &cancellation,
        &mut NoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    output::write_json(&plan, "normalized plan")
}
