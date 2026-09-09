use std::collections::BTreeMap;
use std::path::Path;

use immich_rs_client::{ArchiveListConfig, ArchiveVisibility, ImmichReadClient};
use immich_rs_core::{ArchiveApplyReport, ArchiveManifest, CancellationToken};
use immich_rs_executor::{
    ArchivePlanningConfig, ArchiveSelection, apply_archive, create_archive_manifest,
};

use crate::ApplicationError;

/// List, deduplicate and bind one immutable archive manifest.
pub async fn plan_archive(
    config: &ArchivePlanningConfig,
    client: &ImmichReadClient,
    cancellation: &CancellationToken,
) -> Result<ArchiveManifest, ApplicationError> {
    let negotiated = client.probe(cancellation).await?;
    let visibilities: &[ArchiveVisibility] = match config.selection {
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
        let remaining = config.max_assets.saturating_sub(assets.len());
        if remaining == 0 {
            return Err(ApplicationError::InvalidRequest(
                "archive asset limit exceeded",
            ));
        }
        let mut selected = client
            .list_archive_assets(
                &negotiated,
                ArchiveListConfig {
                    visibility: *visibility,
                    include_trashed: config.include_trashed,
                    page_size: config.page_size,
                    max_assets: remaining,
                },
                cancellation,
            )
            .await?;
        assets.append(&mut selected);
    }
    let mut unique = BTreeMap::new();
    for asset in assets {
        if unique
            .insert(asset.asset_id.clone(), asset.clone())
            .is_some_and(|previous| previous != asset)
        {
            return Err(ApplicationError::Invariant(
                "archive inventory facts changed",
            ));
        }
    }
    Ok(create_archive_manifest(
        unique.into_values().collect(),
        negotiated.compatibility().clone(),
        config,
    )?)
}

/// Probe and execute one archive manifest without a mutation-capable client.
pub async fn apply_archive_manifest(
    manifest: &ArchiveManifest,
    destination: &Path,
    client: &ImmichReadClient,
    cancellation: &CancellationToken,
) -> Result<ArchiveApplyReport, ApplicationError> {
    let negotiated = client.probe(cancellation).await?;
    Ok(apply_archive(manifest, destination, client, &negotiated, cancellation).await?)
}
