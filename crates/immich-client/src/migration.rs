use std::collections::BTreeSet;

use immich_rs_core::{CancellationToken, GeoCoordinates, MediaKind, NormalizedMetadata};
use sha2::Digest as _;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::models::{
    ArchiveAssetResponse, ArchiveSearchRequest, ArchiveSearchResponse, AssetTypeResponse,
    MigrationAlbumResponse,
};
use crate::read::check_cancelled;
use crate::response::{bounded_json, classify_transport};
use crate::{ClientError, ClientErrorClass, ImmichReadClient, NegotiatedServer};

/// Bounded remote inventory limits for one migration plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationListConfig {
    /// Search page size.
    pub page_size: usize,
    /// Maximum accepted timeline assets, including linked Live Photo videos.
    pub max_assets: usize,
    /// Maximum accepted owned albums.
    pub max_albums: usize,
    /// Maximum accepted in-scope membership facts across albums.
    pub max_album_memberships: usize,
}

impl Default for MigrationListConfig {
    fn default() -> Self {
        Self {
            page_size: 100,
            max_assets: 100_000,
            max_albums: 10_000,
            max_album_memberships: 1_000_000,
        }
    }
}

impl MigrationListConfig {
    fn validate(self) -> Result<Self, ClientError> {
        (matches!(self.page_size, 1..=1_000)
            && self.max_assets > 0
            && self.max_albums > 0
            && self.max_album_memberships > 0)
            .then_some(self)
            .ok_or_else(protocol)
    }
}

/// Supported source facts for one migration asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMigrationAsset {
    /// Source asset UUID.
    pub asset_id: String,
    /// Portable original filename.
    pub original_file_name: String,
    /// Original media family.
    pub media_kind: MediaKind,
    /// Source-declared original length.
    pub byte_len: u64,
    /// Lowercase hexadecimal SHA-1.
    pub checksum_sha1: String,
    /// File creation instant in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// File modification instant in Unix milliseconds.
    pub modified_at_unix_ms: i64,
    /// Supported source metadata excluding albums.
    pub normalized_metadata: Option<NormalizedMetadata>,
    /// Related motion-video UUID for a Live Photo image.
    pub live_photo_video_id: Option<String>,
}

/// One owned album projected onto in-scope timeline asset IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteOwnedAlbum {
    /// Source album UUID, used only in the source fingerprint.
    pub album_id: String,
    /// Exact album name.
    pub name: String,
    /// Sorted in-scope source asset UUIDs.
    pub asset_ids: Vec<String>,
}

/// Complete bounded source inventory for migration planning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMigrationInventory {
    /// Assets sorted by source UUID.
    pub assets: Vec<RemoteMigrationAsset>,
    /// Owned albums sorted by exact name and UUID.
    pub albums: Vec<RemoteOwnedAlbum>,
}

impl ImmichReadClient {
    /// Read only the declared migration matrix from a source Immich server.
    pub async fn migration_inventory(
        &self,
        negotiated: &NegotiatedServer,
        config: MigrationListConfig,
        cancellation: &CancellationToken,
    ) -> Result<RemoteMigrationInventory, ClientError> {
        let config = config.validate()?;
        let archive = self
            .list_archive_assets(
                negotiated,
                crate::ArchiveListConfig {
                    visibility: crate::ArchiveVisibility::Timeline,
                    include_trashed: false,
                    page_size: config.page_size,
                    max_assets: config.max_assets,
                },
                cancellation,
            )
            .await?;
        let mut assets = Vec::with_capacity(archive.len());
        for expected in archive {
            check_cancelled(cancellation)?;
            let response: ArchiveAssetResponse = self
                .get_json(&format!("api/assets/{}", expected.asset_id))
                .await?;
            let asset = migration_asset(response)?;
            if asset.asset_id != expected.asset_id
                || asset.original_file_name != expected.original_file_name
                || asset.media_kind != expected.media_kind
                || asset.byte_len != expected.byte_len
                || asset.checksum_sha1 != expected.checksum_sha1
            {
                return Err(protocol());
            }
            assets.push(asset);
        }
        assets.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        if !assets
            .windows(2)
            .all(|pair| pair[0].asset_id < pair[1].asset_id)
        {
            return Err(protocol());
        }
        let allowed = assets
            .iter()
            .map(|asset| asset.asset_id.as_str())
            .collect::<BTreeSet<_>>();
        let albums = self
            .migration_albums(negotiated, config, &allowed, cancellation)
            .await?;
        Ok(RemoteMigrationInventory { assets, albums })
    }

    async fn migration_albums(
        &self,
        negotiated: &NegotiatedServer,
        config: MigrationListConfig,
        allowed: &BTreeSet<&str>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RemoteOwnedAlbum>, ClientError> {
        self.validate_migration_binding(negotiated)?;
        check_cancelled(cancellation)?;
        let url = self
            .endpoint
            .api_url("api/albums?isOwned=true")
            .map_err(|_| protocol())?;
        let response = self
            .http
            .get(url)
            .header("x-api-key", self.api_key.header())
            .send()
            .await
            .map_err(|error| classify_transport(&error))?;
        let headers: Vec<MigrationAlbumResponse> = bounded_json(
            response,
            self.config.max_response_bytes,
            self.config.retry_after_cap,
        )
        .await?;
        if headers.len() > config.max_albums {
            return Err(protocol());
        }
        let mut memberships = 0_usize;
        let mut albums = Vec::new();
        for header in headers {
            check_cancelled(cancellation)?;
            if !uuid_v4(&header.id)
                || !valid_album_name(&header.album_name)
                || header.asset_count > config.max_album_memberships as u64
            {
                return Err(protocol());
            }
            let mut asset_ids = self
                .migration_album_assets(&header.id, config, cancellation)
                .await?;
            asset_ids.retain(|asset_id| allowed.contains(asset_id.as_str()));
            asset_ids.sort();
            asset_ids.dedup();
            memberships = memberships.saturating_add(asset_ids.len());
            if memberships > config.max_album_memberships {
                return Err(protocol());
            }
            if !asset_ids.is_empty() {
                albums.push(RemoteOwnedAlbum {
                    album_id: header.id,
                    name: header.album_name,
                    asset_ids,
                });
            }
        }
        albums.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.album_id.cmp(&right.album_id))
        });
        Ok(albums)
    }

    async fn migration_album_assets(
        &self,
        album_id: &str,
        config: MigrationListConfig,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, ClientError> {
        let album_ids = [album_id.to_owned()];
        let mut page = 1_u64;
        let mut result = Vec::new();
        loop {
            check_cancelled(cancellation)?;
            let response: ArchiveSearchResponse = self
                .post_json(
                    "api/search/metadata",
                    &ArchiveSearchRequest {
                        page,
                        size: config.page_size,
                        order: "asc",
                        with_exif: false,
                        album_ids: Some(&album_ids),
                        visibility: "timeline",
                        with_deleted: None,
                    },
                    cancellation,
                )
                .await?;
            if response.assets.count != response.assets.items.len() as u64 {
                return Err(protocol());
            }
            for asset in response.assets.items {
                if result.len() >= config.max_album_memberships || !uuid_v4(&asset.id) {
                    return Err(protocol());
                }
                result.push(asset.id);
            }
            match response.assets.next_page {
                Some(next) if response.assets.count > 0 => {
                    let next = next.parse::<u64>().map_err(|_| protocol())?;
                    if next != page.saturating_add(1) {
                        return Err(protocol());
                    }
                    page = next;
                }
                Some(_) => return Err(protocol()),
                None => break,
            }
        }
        Ok(result)
    }

    fn validate_migration_binding(&self, negotiated: &NegotiatedServer) -> Result<(), ClientError> {
        let observed = self.endpoint.canonical_origin();
        let expected = negotiated.migration_server().origin_sha256;
        let observed = format!("{:x}", sha2::Sha256::digest(observed.as_bytes()));
        (observed == expected).then_some(()).ok_or_else(protocol)
    }
}

pub fn migration_asset(item: ArchiveAssetResponse) -> Result<RemoteMigrationAsset, ClientError> {
    let media_kind = match item.r#type {
        AssetTypeResponse::Image => MediaKind::Image,
        AssetTypeResponse::Video => MediaKind::Video,
        AssetTypeResponse::Audio | AssetTypeResponse::Other => return Err(protocol()),
    };
    let checksum_sha1 = decode_sha1(&item.checksum)?;
    let exif = item.exif_info.ok_or_else(protocol)?;
    let byte_len = exif
        .file_size_in_byte
        .filter(|size| *size > 0)
        .ok_or_else(protocol)?;
    let created = timestamp_millis(item.file_created_at.as_deref().ok_or_else(protocol)?)?;
    let modified = timestamp_millis(item.file_modified_at.as_deref().ok_or_else(protocol)?)?;
    let normalized_metadata = normalized_metadata(
        exif.description,
        exif.date_time_original
            .as_deref()
            .or(item.file_created_at.as_deref()),
        exif.latitude,
        exif.longitude,
    )?;
    Ok(RemoteMigrationAsset {
        asset_id: item.id,
        original_file_name: item.original_file_name,
        media_kind,
        byte_len,
        checksum_sha1,
        created_at_unix_ms: created,
        modified_at_unix_ms: modified,
        normalized_metadata,
        live_photo_video_id: item.live_photo_video_id,
    })
}

pub fn normalized_metadata(
    description: Option<String>,
    taken_at: Option<&str>,
    latitude: Option<f64>,
    longitude: Option<f64>,
) -> Result<Option<NormalizedMetadata>, ClientError> {
    let description = description.filter(|value| !value.is_empty());
    let taken_at_utc = taken_at.map(canonical_timestamp).transpose()?;
    let location = match (latitude, longitude) {
        (None, None) => None,
        (Some(latitude), Some(longitude)) if latitude.is_finite() && longitude.is_finite() => {
            Some(GeoCoordinates {
                latitude: canonical_decimal(latitude),
                longitude: canonical_decimal(longitude),
            })
        }
        _ => return Err(protocol()),
    };
    let metadata = NormalizedMetadata {
        description,
        taken_at_utc,
        location,
        albums: Vec::new(),
    };
    Ok((metadata != NormalizedMetadata::default()).then_some(metadata))
}

fn timestamp_millis(value: &str) -> Result<i64, ClientError> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339).map_err(|_| protocol())?;
    i64::try_from(timestamp.unix_timestamp_nanos() / 1_000_000).map_err(|_| protocol())
}

fn canonical_timestamp(value: &str) -> Result<String, ClientError> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339).map_err(|_| protocol())?;
    timestamp
        .replace_nanosecond(0)
        .map_err(|_| protocol())?
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| protocol())
}

fn canonical_decimal(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

fn decode_sha1(value: &str) -> Result<String, ClientError> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| protocol())?;
    if bytes.len() != 20 {
        return Err(protocol());
    }
    Ok(crate::archive::lowercase_hex(&bytes))
}

fn uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes.get(14) == Some(&b'4')
        && bytes
            .get(19)
            .is_some_and(|byte| matches!(byte, b'8' | b'9' | b'a' | b'b'))
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || (byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

fn valid_album_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
}

const fn protocol() -> ClientError {
    ClientError::new(ClientErrorClass::Protocol)
}
