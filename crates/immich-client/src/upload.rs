use std::fmt::{self, Debug, Formatter};
use std::path::Path;
use std::time::Duration;

use immich_rs_core::{Cancellation, CancellationToken, ServerCompatibility};
use reqwest::multipart::{Form, Part};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::models::{
    BulkCheckAction, BulkCheckAsset, BulkCheckRequest, BulkCheckResponse, UploadResponse,
    UploadStatus,
};
use crate::read::ImmichReadClient;
use crate::response::{bounded_json, classify_transport};
use crate::{ClientError, ClientErrorClass};

/// Result of Immich's checksum duplicate preflight.
#[derive(Clone, Eq, PartialEq)]
pub enum DuplicateCheck {
    /// Immich permits a new upload.
    Accept,
    /// Immich already has this checksum.
    Duplicate(String),
}

impl Debug for DuplicateCheck {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept => formatter.write_str("Accept"),
            Self::Duplicate(_) => formatter.write_str("Duplicate([REDACTED])"),
        }
    }
}

impl DuplicateCheck {
    /// Return the existing asset ID only for executor dependency handling.
    #[must_use]
    pub fn asset_id(&self) -> Option<&str> {
        match self {
            Self::Accept => None,
            Self::Duplicate(asset_id) => Some(asset_id),
        }
    }
}

/// Definite result returned by an asset upload response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadResult {
    /// Immich created a new asset.
    Created,
    /// Immich converged on an existing asset.
    Duplicate,
}

/// Local inputs for one bounded streaming multipart upload.
pub struct UploadRequest<'a> {
    /// Stable client operation ID.
    pub operation_id: &'a str,
    /// Portable filename only, never an absolute path.
    pub file_name: &'a str,
    /// Native media path resolved by the source adapter.
    pub media_path: &'a Path,
    /// Exact media size verified by the executor.
    pub media_len: u64,
    /// Base64 SHA-1 understood by Immich.
    pub sha1_base64: &'a str,
    /// Captured creation instant in Unix milliseconds.
    pub created_at_unix_ms: i64,
    /// Captured modification instant in Unix milliseconds.
    pub modified_at_unix_ms: i64,
    /// Optional native XMP path and exact length.
    pub xmp: Option<(&'a Path, u64)>,
    /// Server asset ID of a previously uploaded live-photo video.
    pub live_photo_video_id: Option<&'a str>,
}

impl Debug for UploadRequest<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UploadRequest")
            .field("operation_id", &"[REDACTED]")
            .field("file_name", &"[REDACTED]")
            .field("media_path", &"[REDACTED]")
            .field("media_len", &self.media_len)
            .field("sha1_base64", &"[REDACTED]")
            .field("has_xmp", &self.xmp.is_some())
            .field("has_live_photo_video", &self.live_photo_video_id.is_some())
            .finish()
    }
}

/// Upload capability constructed only after successful negotiation.
pub struct ImmichUploadClient {
    read: ImmichReadClient,
    compatibility: ServerCompatibility,
}

impl ImmichUploadClient {
    pub(crate) const fn from_negotiated(
        read: ImmichReadClient,
        compatibility: ServerCompatibility,
    ) -> Self {
        Self {
            read,
            compatibility,
        }
    }

    /// Return the authenticated server binding that created this capability.
    #[must_use]
    pub const fn compatibility(&self) -> &ServerCompatibility {
        &self.compatibility
    }

    /// Ask Immich whether a checksum should be uploaded.
    pub async fn duplicate_check(
        &self,
        operation_id: &str,
        sha1_base64: &str,
        cancellation: &CancellationToken,
    ) -> Result<DuplicateCheck, ClientError> {
        check_cancelled(cancellation)?;
        let request = BulkCheckRequest {
            assets: [BulkCheckAsset {
                id: operation_id,
                checksum: sha1_base64,
            }],
        };
        let url = self.api_url("api/assets/bulk-upload-check")?;
        let response = cancellable(
            self.read
                .http
                .post(url)
                .header("x-api-key", self.read.api_key.header())
                .json(&request)
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        let body: BulkCheckResponse = bounded_json(
            response,
            self.read.config.max_response_bytes,
            self.read.config.retry_after_cap,
        )
        .await?;
        let [result] = body.results.as_slice() else {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        };
        if result.id != operation_id {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        match (result.action, result.asset_id.as_deref()) {
            (BulkCheckAction::Accept, None) => Ok(DuplicateCheck::Accept),
            (BulkCheckAction::Reject, Some(asset_id))
                if !asset_id.is_empty() && asset_id.len() <= 4_096 =>
            {
                Ok(DuplicateCheck::Duplicate(asset_id.to_owned()))
            }
            _ => Err(ClientError::new(ClientErrorClass::Protocol)),
        }
    }

    /// Stream one verified media asset and optional XMP sidecar to Immich.
    pub async fn upload(
        &self,
        request: &UploadRequest<'_>,
        cancellation: &CancellationToken,
    ) -> Result<(UploadResult, String), ClientError> {
        check_cancelled(cancellation)?;
        validate_upload_request(request)?;
        let form = self.multipart(request).await?;
        let url = self.api_url("api/assets")?;
        let response = cancellable(
            self.read
                .http
                .post(url)
                .header("x-api-key", self.read.api_key.header())
                .header("x-immich-checksum", request.sha1_base64)
                .multipart(form)
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        let body: UploadResponse = bounded_json(
            response,
            self.read.config.max_response_bytes,
            self.read.config.retry_after_cap,
        )
        .await?;
        if body.id.is_empty() || body.id.len() > 4_096 {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        let result = match body.status {
            UploadStatus::Created => UploadResult::Created,
            UploadStatus::Duplicate => UploadResult::Duplicate,
        };
        Ok((result, body.id))
    }

    async fn multipart(&self, request: &UploadRequest<'_>) -> Result<Form, ClientError> {
        let media = streaming_part(
            request.media_path,
            request.media_len,
            request.file_name,
            self.read.config.upload_buffer_bytes,
        )
        .await?;
        let mut form = Form::new()
            .part("assetData", media)
            .text("deviceAssetId", request.operation_id.to_owned())
            .text("deviceId", "immich-rs".to_owned())
            .text(
                "fileCreatedAt",
                format_timestamp(request.created_at_unix_ms)?,
            )
            .text(
                "fileModifiedAt",
                format_timestamp(request.modified_at_unix_ms)?,
            )
            .text("isFavorite", "false".to_owned());
        if let Some((path, len)) = request.xmp {
            let sidecar = streaming_part(
                path,
                len,
                "metadata.xmp",
                self.read.config.upload_buffer_bytes,
            )
            .await?;
            form = form.part("sidecarData", sidecar);
        }
        if let Some(asset_id) = request.live_photo_video_id {
            form = form.text("livePhotoVideoId", asset_id.to_owned());
        }
        Ok(form)
    }

    fn api_url(&self, path: &str) -> Result<url::Url, ClientError> {
        self.read
            .endpoint
            .api_url(path)
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))
    }
}

impl Debug for ImmichUploadClient {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmichUploadClient")
            .field("read", &self.read)
            .field("compatibility", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

async fn streaming_part(
    path: &Path,
    len: u64,
    file_name: &str,
    buffer_bytes: usize,
) -> Result<Part, ClientError> {
    let file = File::open(path)
        .await
        .map_err(|_| ClientError::new(ClientErrorClass::Source))?;
    let stream = ReaderStream::with_capacity(file, buffer_bytes);
    Ok(
        Part::stream_with_length(reqwest::Body::wrap_stream(stream), len)
            .file_name(file_name.to_owned()),
    )
}

fn format_timestamp(unix_ms: i64) -> Result<String, ClientError> {
    let nanos = i128::from(unix_ms)
        .checked_mul(1_000_000)
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))?;
    let timestamp = OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
    timestamp
        .format(&Rfc3339)
        .map_err(|_| ClientError::new(ClientErrorClass::Protocol))
}

fn validate_upload_request(request: &UploadRequest<'_>) -> Result<(), ClientError> {
    let valid = !request.operation_id.is_empty()
        && !request.file_name.is_empty()
        && !request.file_name.contains(['/', '\\'])
        && request.media_len > 0
        && !request.sha1_base64.is_empty()
        && request.sha1_base64.len() <= 128
        && request
            .live_photo_video_id
            .is_none_or(|id| !id.is_empty() && id.len() <= 4_096)
        && request.xmp.is_none_or(|(_, len)| len > 0);
    valid
        .then_some(())
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ClientError> {
    if cancellation.is_cancelled() {
        Err(ClientError::new(ClientErrorClass::Cancelled))
    } else {
        Ok(())
    }
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
