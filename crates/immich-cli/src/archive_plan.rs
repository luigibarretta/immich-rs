use std::collections::BTreeMap;

use immich_rs_client::{ArchiveListConfig, ArchiveVisibility};
use immich_rs_core::CancellationToken;
use immich_rs_executor::{ArchiveSelection, create_archive_manifest};

use crate::args::ArchivePlanRequest;
use crate::failure::CliFailure;
use crate::{network, output, signal};

pub async fn run(request: ArchivePlanRequest) -> Result<(), CliFailure> {
    let cancellation = CancellationToken::default();
    signal::install(cancellation.clone())?;
    let client = network::archive_client(&request.server, request.production_read)?;
    let negotiated = client
        .probe(&cancellation)
        .await
        .map_err(CliFailure::from_client)?;
    let visibilities: &[ArchiveVisibility] = match request.config.selection {
        ArchiveSelection::Timeline => &[ArchiveVisibility::Timeline],
        ArchiveSelection::Archive => &[ArchiveVisibility::Archive],
        ArchiveSelection::Hidden => &[ArchiveVisibility::Hidden],
        ArchiveSelection::All => &[
            ArchiveVisibility::Timeline,
            ArchiveVisibility::Archive,
            ArchiveVisibility::Hidden,
        ],
    };
    let mut assets = Vec::new();
    for visibility in visibilities {
        let remaining = request.config.max_assets.saturating_sub(assets.len());
        if remaining == 0 {
            return Err(CliFailure::usage("archive asset limit exceeded"));
        }
        let mut selected = client
            .list_archive_assets(
                &negotiated,
                ArchiveListConfig {
                    visibility: *visibility,
                    include_trashed: request.config.include_trashed,
                    page_size: request.config.page_size,
                    max_assets: remaining,
                },
                &cancellation,
            )
            .await
            .map_err(CliFailure::from_client)?;
        assets.append(&mut selected);
    }
    let mut unique = BTreeMap::new();
    for asset in assets {
        if unique
            .insert(asset.asset_id.clone(), asset.clone())
            .is_some_and(|previous| previous != asset)
        {
            return Err(CliFailure::invariant("archive inventory facts changed"));
        }
    }
    let manifest = create_archive_manifest(
        unique.into_values().collect(),
        negotiated.compatibility().clone(),
        &request.config,
    )
    .map_err(CliFailure::from_executor)?;
    output::write_json(&manifest, "archive manifest")
}
