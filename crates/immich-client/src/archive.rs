use std::fmt::{self, Debug, Formatter};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_core::{Cancellation, CancellationToken, MediaKind};
use sha2::{Digest, Sha256};

use crate::models::{
    ArchiveAssetResponse, ArchiveSearchRequest, ArchiveSearchResponse, AssetTypeResponse,
};
use crate::read::check_cancelled;
use crate::response::{bounded_json, classify_transport, status_error};
use crate::{ClientError, ClientErrorClass, ImmichReadClient, NegotiatedServer};

/// One explicit Immich asset visibility selected for archive planning.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ArchiveVisibility {
    /// Normal timeline assets.
    Timeline,
    /// Archived assets.
    Archive,
    /// Hidden assets.
    Hidden,
}

impl ArchiveVisibility {
    const fn api_value(self) -> &'static str {
        match self {
            Self::Timeline => "timeline",
            Self::Archive => "archive",
            Self::Hidden => "hidden",
        }
    }
}

/// Explicit pagination and selection limits for read-only archive inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveListConfig {
    /// Visibility selected by one paginated search.
    pub visibility: ArchiveVisibility,
    /// Include trashed assets in this visibility.
    pub include_trashed: bool,
    /// Page size sent to Immich.
    pub page_size: usize,
    /// Maximum assets accepted before failing closed.
    pub max_assets: usize,
}

impl Default for ArchiveListConfig {
    fn default() -> Self {
        Self {
            visibility: ArchiveVisibility::Timeline,
            include_trashed: false,
            page_size: 100,
            max_assets: 100_000,
        }
    }
}

impl ArchiveListConfig {
    fn validate(self) -> Result<Self, ClientError> {
        (matches!(self.page_size, 1..=1_000) && self.max_assets > 0)
            .then_some(self)
            .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
    }
}

/// Source facts returned by the read-only Immich inventory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteArchiveAsset {
    /// Canonical server asset UUID.
    pub asset_id: String,
    /// Original filename from Immich.
    pub original_file_name: String,
    /// Source media family.
    pub media_kind: MediaKind,
    /// Original byte length from EXIF metadata.
    pub byte_len: u64,
    /// Lowercase hexadecimal SHA-1.
    pub checksum_sha1: String,
}

impl ImmichReadClient {
    /// Enumerate one visibility through bounded read-only pages.
    pub async fn list_archive_assets(
        &self,
        negotiated: &NegotiatedServer,
        config: ArchiveListConfig,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RemoteArchiveAsset>, ClientError> {
        self.validate_binding(negotiated)?;
        let config = config.validate()?;
        let mut assets = Vec::new();
        let mut related_ids = Vec::new();
        let mut page = 1_u64;
        loop {
            check_cancelled(cancellation)?;
            let request = ArchiveSearchRequest {
                page,
                size: config.page_size,
                order: "asc",
                with_exif: true,
                album_ids: None,
                visibility: config.visibility.api_value(),
                with_deleted: config.include_trashed.then_some(true),
            };
            let response: ArchiveSearchResponse = self
                .post_json("api/search/metadata", &request, cancellation)
                .await?;
            if response.assets.total > config.max_assets as u64 {
                return Err(ClientError::new(ClientErrorClass::Protocol));
            }
            if response.assets.count != response.assets.items.len() as u64 {
                return Err(ClientError::new(ClientErrorClass::Protocol));
            }
            for item in response.assets.items {
                if assets.len() >= config.max_assets {
                    return Err(ClientError::new(ClientErrorClass::Protocol));
                }
                let (asset, related_id) = convert_asset(item)?;
                assets.push(asset);
                if let Some(identifier) = related_id {
                    related_ids.push(identifier);
                }
            }
            if let Some(next_page) = response.assets.next_page {
                if response.assets.count == 0 {
                    return Err(ClientError::new(ClientErrorClass::Protocol));
                }
                let parsed = next_page
                    .parse::<u64>()
                    .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
                if parsed != page.saturating_add(1) {
                    return Err(ClientError::new(ClientErrorClass::Protocol));
                }
                page = parsed;
            } else {
                break;
            }
        }
        related_ids.sort();
        related_ids.dedup();
        for identifier in related_ids {
            if assets.iter().any(|asset| asset.asset_id == identifier) {
                continue;
            }
            if assets.len() >= config.max_assets {
                return Err(ClientError::new(ClientErrorClass::Protocol));
            }
            check_cancelled(cancellation)?;
            let item: ArchiveAssetResponse =
                self.get_json(&format!("api/assets/{identifier}")).await?;
            let (asset, _) = convert_asset(item)?;
            if asset.asset_id != identifier {
                return Err(ClientError::new(ClientErrorClass::Protocol));
            }
            assets.push(asset);
        }
        Ok(assets)
    }

    /// Open one authenticated original download without buffering its body.
    pub async fn download_original(
        &self,
        negotiated: &NegotiatedServer,
        asset_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<ArchiveDownload, ClientError> {
        self.validate_binding(negotiated)?;
        check_cancelled(cancellation)?;
        let url = self
            .endpoint
            .api_url(&format!("api/assets/{asset_id}/original"))
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
        let response = cancellable(
            self.http
                .get(url)
                .header("x-api-key", self.api_key.header())
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        if !response.status().is_success() {
            return Err(status_error(&response, self.config.retry_after_cap));
        }
        Ok(ArchiveDownload { response })
    }

    fn validate_binding(&self, negotiated: &NegotiatedServer) -> Result<(), ClientError> {
        let origin_sha256 = format!(
            "{:x}",
            Sha256::digest(self.endpoint.canonical_origin().as_bytes())
        );
        (origin_sha256 == negotiated.origin_sha256)
            .then_some(())
            .ok_or_else(|| ClientError::new(ClientErrorClass::Compatibility))
    }

    pub(crate) async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl serde::Serialize + Sync),
        cancellation: &CancellationToken,
    ) -> Result<T, ClientError> {
        let url = self
            .endpoint
            .api_url(path)
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
        let response = cancellable(
            self.http
                .post(url)
                .header("x-api-key", self.api_key.header())
                .json(body)
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        bounded_json(
            response,
            self.config.max_response_bytes,
            self.config.retry_after_cap,
        )
        .await
    }
}

/// Bounded-chunk view of one original response body.
pub struct ArchiveDownload {
    response: reqwest::Response,
}

impl ArchiveDownload {
    /// Server-declared response length, when present.
    #[must_use]
    pub fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }

    /// Read one transport-bounded chunk with cooperative cancellation.
    pub async fn next_chunk(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<Option<Vec<u8>>, ClientError> {
        cancellable(self.response.chunk(), cancellation)
            .await?
            .map_err(|error| classify_transport(&error))
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
    }
}

impl Debug for ArchiveDownload {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ArchiveDownload([REDACTED])")
    }
}

fn convert_asset(
    item: ArchiveAssetResponse,
) -> Result<(RemoteArchiveAsset, Option<String>), ClientError> {
    let media_kind = match item.r#type {
        AssetTypeResponse::Image => MediaKind::Image,
        AssetTypeResponse::Video => MediaKind::Video,
        AssetTypeResponse::Audio | AssetTypeResponse::Other => {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
    };
    let decoded = STANDARD
        .decode(item.checksum)
        .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
    if decoded.len() != 20 {
        return Err(ClientError::new(ClientErrorClass::Protocol));
    }
    let byte_len = item
        .exif_info
        .and_then(|exif| exif.file_size_in_byte)
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))?;
    let checksum_sha1 = lowercase_hex(&decoded);
    Ok((
        RemoteArchiveAsset {
            asset_id: item.id,
            original_file_name: item.original_file_name,
            media_kind,
            byte_len,
            checksum_sha1,
        },
        item.live_photo_video_id,
    ))
}

pub fn lowercase_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

async fn cancellable<T>(
    future: impl Future<Output = T>,
    cancellation: &CancellationToken,
) -> Result<T, ClientError> {
    tokio::select! {
        output = future => Ok(output),
        () = wait_cancelled(cancellation) => Err(ClientError::new(ClientErrorClass::Cancelled)),
    }
}

async fn wait_cancelled(cancellation: &CancellationToken) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
