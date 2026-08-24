use std::collections::BTreeMap;

use immich_rs_client::{
    ClientError, ClientErrorClass, ImmichReadClient, MigrationListConfig, NegotiatedServer,
    RemoteMigrationAsset, RemoteMigrationInventory,
};
use immich_rs_core::{
    CancellationToken, MIGRATION_PLAN_SCHEMA_VERSION, MediaKind, MigrationAlbum, MigrationAsset,
    MigrationPlan, MigrationPlanSummary, MigrationServer, UploadRole,
};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;

use crate::{ExecutorError, ExecutorErrorClass};

/// Resource limits bound into an Immich-to-Immich migration plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationPlanningConfig {
    /// Remote inventory and album limits.
    pub inventory: MigrationListConfig,
    /// Maximum accepted bytes for one original.
    pub max_asset_bytes: u64,
    /// Maximum accepted bytes across the source inventory.
    pub max_total_bytes: u64,
}

impl Default for MigrationPlanningConfig {
    fn default() -> Self {
        Self {
            inventory: MigrationListConfig::default(),
            max_asset_bytes: 1024_u64.pow(4),
            max_total_bytes: 16 * 1024_u64.pow(4),
        }
    }
}

impl MigrationPlanningConfig {
    /// Validate all migration inventory and byte limits.
    pub fn validate(self) -> Result<Self, ExecutorError> {
        let inventory = self.inventory;
        let valid_inventory = matches!(inventory.page_size, 1..=1_000)
            && inventory.max_assets > 0
            && inventory.max_albums > 0
            && inventory.max_album_memberships > 0;
        (valid_inventory
            && self.max_asset_bytes > 0
            && self.max_total_bytes >= self.max_asset_bytes)
            .then_some(self)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))
    }

    fn identity(self) -> String {
        let inventory = self.inventory;
        let value = format!(
            "migration-config-v1\0{}\0{}\0{}\0{}\0{}\0{}",
            inventory.page_size,
            inventory.max_assets,
            inventory.max_albums,
            inventory.max_album_memberships,
            self.max_asset_bytes,
            self.max_total_bytes
        );
        format!("{:x}", Sha256::digest(value.as_bytes()))
    }
}

/// Inventory and verify every declared source original without retaining media.
pub async fn create_migration_plan(
    source_client: &ImmichReadClient,
    source_server: &NegotiatedServer,
    destination_server: &NegotiatedServer,
    config: MigrationPlanningConfig,
    cancellation: &CancellationToken,
) -> Result<MigrationPlan, ExecutorError> {
    let config = config.validate()?;
    let source = source_server.migration_server();
    let destination = destination_server.migration_server();
    if source.origin_sha256 == destination.origin_sha256
        || source.compatibility.identity_sha256 == destination.compatibility.identity_sha256
    {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
    }
    let inventory = source_client
        .migration_inventory(source_server, config.inventory, cancellation)
        .await
        .map_err(map_client_error)?;
    let declared_total = inventory
        .assets
        .iter()
        .try_fold(0_u64, |total, asset| total.checked_add(asset.byte_len))
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidConfiguration))?;
    if declared_total > config.max_total_bytes
        || inventory
            .assets
            .iter()
            .any(|asset| asset.byte_len > config.max_asset_bytes)
    {
        return Err(ExecutorError::new(ExecutorErrorClass::InvalidConfiguration));
    }
    let mut hashes = BTreeMap::new();
    for asset in &inventory.assets {
        let digest = verify_original(
            source_client,
            source_server,
            asset,
            config.max_asset_bytes,
            cancellation,
        )
        .await?;
        hashes.insert(asset.asset_id.clone(), digest);
    }
    assemble_migration_plan(inventory, source, destination, &hashes, config)
}

pub fn assemble_migration_plan(
    inventory: RemoteMigrationInventory,
    source_server: MigrationServer,
    destination_server: MigrationServer,
    content_sha256: &BTreeMap<String, String>,
    config: MigrationPlanningConfig,
) -> Result<MigrationPlan, ExecutorError> {
    let config = config.validate()?;
    let operation_ids = inventory
        .assets
        .iter()
        .map(|asset| {
            let body = content_sha256
                .get(&asset.asset_id)
                .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
            Ok((
                asset.asset_id.clone(),
                operation_id(&source_server, asset, body)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, ExecutorError>>()?;
    if operation_ids.len() != inventory.assets.len()
        || content_sha256.len() != inventory.assets.len()
    {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    let roles = live_photo_roles(&inventory.assets, &operation_ids)?;
    let assets = inventory
        .assets
        .iter()
        .map(|asset| {
            Ok(MigrationAsset {
                source_asset_id: asset.asset_id.clone(),
                operation_id: operation_ids
                    .get(&asset.asset_id)
                    .ok_or_else(invariant)?
                    .clone(),
                original_file_name: asset.original_file_name.clone(),
                media_kind: asset.media_kind,
                byte_len: asset.byte_len,
                checksum_sha1: asset.checksum_sha1.clone(),
                content_sha256: content_sha256
                    .get(&asset.asset_id)
                    .ok_or_else(invariant)?
                    .clone(),
                created_at_unix_ms: asset.created_at_unix_ms,
                modified_at_unix_ms: asset.modified_at_unix_ms,
                normalized_metadata: asset.normalized_metadata.clone(),
                role: roles.get(&asset.asset_id).ok_or_else(invariant)?.clone(),
            })
        })
        .collect::<Result<Vec<_>, ExecutorError>>()?;
    let albums = migration_albums(inventory, &operation_ids)?;
    let source_fingerprint_sha256 = source_fingerprint(&source_server, &assets, &albums)?;
    let summary = summary(&assets, &albums);
    let plan = MigrationPlan {
        schema_version: MIGRATION_PLAN_SCHEMA_VERSION,
        source_server,
        destination_server,
        source_fingerprint_sha256,
        configuration_sha256: config.identity(),
        assets,
        albums,
        summary,
    };
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    Ok(plan)
}

async fn verify_original(
    client: &ImmichReadClient,
    server: &NegotiatedServer,
    asset: &RemoteMigrationAsset,
    max_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<String, ExecutorError> {
    let mut download = client
        .download_original(server, &asset.asset_id, cancellation)
        .await
        .map_err(map_client_error)?;
    if download
        .content_length()
        .is_some_and(|length| length != asset.byte_len || length > max_bytes)
    {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut bytes = 0_u64;
    while let Some(chunk) = download
        .next_chunk(cancellation)
        .await
        .map_err(map_client_error)?
    {
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::SourceChanged))?;
        if bytes > asset.byte_len || bytes > max_bytes {
            return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
        }
        sha1.update(&chunk);
        sha256.update(&chunk);
    }
    if bytes != asset.byte_len || format!("{:x}", sha1.finalize()) != asset.checksum_sha1 {
        return Err(ExecutorError::new(ExecutorErrorClass::SourceChanged));
    }
    Ok(format!("{:x}", sha256.finalize()))
}

fn live_photo_roles(
    assets: &[RemoteMigrationAsset],
    operation_ids: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, UploadRole>, ExecutorError> {
    let by_id = assets
        .iter()
        .map(|asset| (asset.asset_id.as_str(), asset))
        .collect::<BTreeMap<_, _>>();
    let mut roles = assets
        .iter()
        .map(|asset| (asset.asset_id.clone(), UploadRole::Standalone))
        .collect::<BTreeMap<_, _>>();
    let mut video_owners = BTreeMap::new();
    for image in assets
        .iter()
        .filter(|asset| asset.live_photo_video_id.is_some())
    {
        let video_id = image.live_photo_video_id.as_deref().ok_or_else(invariant)?;
        let video = by_id
            .get(video_id)
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::UnsupportedMetadata))?;
        if image.media_kind != MediaKind::Image
            || video.media_kind != MediaKind::Video
            || video.live_photo_video_id.is_some()
            || video_owners
                .insert(video_id, image.asset_id.as_str())
                .is_some()
        {
            return Err(ExecutorError::new(ExecutorErrorClass::UnsupportedMetadata));
        }
        let pair_id = pair_id(&image.asset_id, video_id);
        roles.insert(
            image.asset_id.clone(),
            UploadRole::LivePhotoImage {
                pair_id: pair_id.clone(),
                video_operation_id: operation_ids.get(video_id).ok_or_else(invariant)?.clone(),
            },
        );
        roles.insert(video_id.to_owned(), UploadRole::LivePhotoVideo { pair_id });
    }
    Ok(roles)
}

fn migration_albums(
    inventory: RemoteMigrationInventory,
    operation_ids: &BTreeMap<String, String>,
) -> Result<Vec<MigrationAlbum>, ExecutorError> {
    let mut albums = Vec::with_capacity(inventory.albums.len());
    for album in inventory.albums {
        let mut members = album
            .asset_ids
            .iter()
            .map(|asset_id| operation_ids.get(asset_id).cloned().ok_or_else(invariant))
            .collect::<Result<Vec<_>, ExecutorError>>()?;
        members.sort();
        members.dedup();
        albums.push(MigrationAlbum {
            name: album.name,
            member_operation_ids: members,
        });
    }
    albums.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(albums)
}

fn operation_id(
    source: &MigrationServer,
    asset: &RemoteMigrationAsset,
    content_sha256: &str,
) -> Result<String, ExecutorError> {
    let metadata = serde_json::to_vec(&asset.normalized_metadata)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    let mut hasher = Sha256::new();
    for bytes in [
        b"immich-migration-operation-v1".as_slice(),
        source.compatibility.identity_sha256.as_bytes(),
        asset.asset_id.as_bytes(),
        asset.original_file_name.as_bytes(),
        asset.checksum_sha1.as_bytes(),
        content_sha256.as_bytes(),
        &metadata,
    ] {
        hasher.update(bytes);
        hasher.update([0]);
    }
    hasher.update(asset.byte_len.to_be_bytes());
    hasher.update(asset.created_at_unix_ms.to_be_bytes());
    hasher.update(asset.modified_at_unix_ms.to_be_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn pair_id(image_id: &str, video_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"immich-migration-live-photo-v1\0");
    hasher.update(image_id.as_bytes());
    hasher.update([0]);
    hasher.update(video_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn source_fingerprint(
    source: &MigrationServer,
    assets: &[MigrationAsset],
    albums: &[MigrationAlbum],
) -> Result<String, ExecutorError> {
    let bytes = serde_json::to_vec(&(source, assets, albums))
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn summary(assets: &[MigrationAsset], albums: &[MigrationAlbum]) -> MigrationPlanSummary {
    let metadata_updates = assets
        .iter()
        .filter(|asset| {
            asset.normalized_metadata.as_ref().is_some_and(|metadata| {
                metadata.description.is_some()
                    || metadata.taken_at_utc.is_some()
                    || metadata.location.is_some()
            })
        })
        .count() as u64;
    let assets_count = assets.len() as u64;
    let album_count = albums.len() as u64;
    MigrationPlanSummary {
        assets: assets_count,
        media_bytes: assets.iter().map(|asset| asset.byte_len).sum(),
        metadata_updates,
        album_creates: album_count,
        album_memberships: album_count,
        max_mutations: assets_count
            .saturating_add(metadata_updates)
            .saturating_add(album_count.saturating_mul(2)),
    }
}

fn map_client_error(error: ClientError) -> ExecutorError {
    if error.class() == ClientErrorClass::Cancelled {
        ExecutorError::new(ExecutorErrorClass::Cancelled)
    } else {
        ExecutorError::new(ExecutorErrorClass::Client)
    }
}

const fn invariant() -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::Invariant)
}
