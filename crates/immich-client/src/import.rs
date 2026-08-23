use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::time::Duration;

use immich_rs_core::{Cancellation, CancellationToken, NormalizedMetadata};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::import_models::{
    AlbumResponse, AssetResponse, BulkIdError, BulkIdResponse, BulkIdsRequest, CreateAlbumRequest,
    UpdateAssetRequest,
};
use crate::response::{bounded_json, classify_transport};
use crate::{ClientError, ClientErrorClass, ImmichUploadClient};

const MAX_ALBUM_ASSETS: usize = 100_000;
const MAX_REMOTE_ID_BYTES: usize = 36;

/// Minimal immutable album identity returned by the import capability.
#[derive(Clone, Eq, PartialEq)]
pub struct RemoteAlbum {
    id: String,
    name: String,
}

impl RemoteAlbum {
    /// Return the server album ID for checkpoint dependencies.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Return the exact album name used for deterministic reconciliation.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Debug for RemoteAlbum {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteAlbum")
            .field("id", &"[REDACTED]")
            .field("name", &"[REDACTED]")
            .finish()
    }
}

/// Disposable import capability for upload, metadata and album effects.
pub struct ImmichImportClient {
    upload: ImmichUploadClient,
}

impl ImmichImportClient {
    pub(crate) const fn new(upload: ImmichUploadClient) -> Self {
        Self { upload }
    }

    /// Return the upload capability used by asset effects.
    #[must_use]
    pub const fn upload(&self) -> &ImmichUploadClient {
        &self.upload
    }

    /// Apply an exact non-empty normalized metadata assignment.
    pub async fn update_asset_metadata(
        &self,
        asset_id: &str,
        metadata: &NormalizedMetadata,
        cancellation: &CancellationToken,
    ) -> Result<(), ClientError> {
        check_cancelled(cancellation)?;
        let request = metadata_request(metadata)?;
        let path = format!("api/assets/{asset_id}");
        let response = cancellable(
            self.upload
                .read
                .http
                .put(self.api_url(&path)?)
                .header("x-api-key", self.upload.read.api_key.header())
                .json(&request)
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        let body: AssetResponse = self.bounded(response).await?;
        if body.id != asset_id || !valid_remote_id(&body.id) {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        Ok(())
    }

    /// List owned albums matching one exact validated name.
    pub async fn find_owned_albums(
        &self,
        name: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RemoteAlbum>, ClientError> {
        check_cancelled(cancellation)?;
        validate_album_name(name)?;
        let mut url = self.api_url("api/albums")?;
        url.query_pairs_mut()
            .append_pair("name", name)
            .append_pair("isOwned", "true");
        let response = cancellable(
            self.upload
                .read
                .http
                .get(url)
                .header("x-api-key", self.upload.read.api_key.header())
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        let albums: Vec<AlbumResponse> = self.bounded(response).await?;
        let mut result = Vec::with_capacity(albums.len());
        for album in albums {
            if album.album_name != name || !valid_remote_id(&album.id) {
                return Err(ClientError::new(ClientErrorClass::Protocol));
            }
            result.push(RemoteAlbum {
                id: album.id,
                name: album.album_name,
            });
        }
        result.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(result)
    }

    /// Create one empty album with an exact validated name.
    pub async fn create_album(
        &self,
        name: &str,
        cancellation: &CancellationToken,
    ) -> Result<RemoteAlbum, ClientError> {
        check_cancelled(cancellation)?;
        validate_album_name(name)?;
        let response = cancellable(
            self.upload
                .read
                .http
                .post(self.api_url("api/albums")?)
                .header("x-api-key", self.upload.read.api_key.header())
                .json(&CreateAlbumRequest { album_name: name })
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        let album: AlbumResponse = self.bounded(response).await?;
        if album.album_name != name || !valid_remote_id(&album.id) {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        Ok(RemoteAlbum {
            id: album.id,
            name: album.album_name,
        })
    }

    /// Add one sorted, unique, bounded asset set to an album.
    pub async fn add_album_assets(
        &self,
        album_id: &str,
        asset_ids: &[&str],
        cancellation: &CancellationToken,
    ) -> Result<(), ClientError> {
        check_cancelled(cancellation)?;
        validate_ids(album_id, asset_ids)?;
        let path = format!("api/albums/{album_id}/assets");
        let response = cancellable(
            self.upload
                .read
                .http
                .put(self.api_url(&path)?)
                .header("x-api-key", self.upload.read.api_key.header())
                .json(&BulkIdsRequest { ids: asset_ids })
                .send(),
            cancellation,
        )
        .await?
        .map_err(|error| classify_transport(&error))?;
        let results: Vec<BulkIdResponse> = self.bounded(response).await?;
        validate_membership_results(asset_ids, results)
    }

    fn api_url(&self, path: &str) -> Result<url::Url, ClientError> {
        self.upload
            .read
            .endpoint
            .api_url(path)
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))
    }

    async fn bounded<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, ClientError> {
        bounded_json(
            response,
            self.upload.read.config.max_response_bytes,
            self.upload.read.config.retry_after_cap,
        )
        .await
    }
}

impl Debug for ImmichImportClient {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmichImportClient")
            .field("upload", &self.upload)
            .finish_non_exhaustive()
    }
}

fn metadata_request(metadata: &NormalizedMetadata) -> Result<UpdateAssetRequest<'_>, ClientError> {
    if metadata.description.as_ref().is_some_and(|value| {
        value.is_empty()
            || value.len() > 16 * 1_024
            || value
                .chars()
                .any(|character| character.is_control() && !"\n\r\t".contains(character))
    }) || metadata
        .taken_at_utc
        .as_ref()
        .is_some_and(|value| !valid_timestamp(value))
    {
        return Err(ClientError::new(ClientErrorClass::Protocol));
    }
    let coordinates = metadata.location.as_ref().map(|location| {
        let latitude = location.latitude.parse::<f64>();
        let longitude = location.longitude.parse::<f64>();
        (latitude, longitude)
    });
    let (latitude, longitude) = match coordinates {
        Some((Ok(latitude), Ok(longitude)))
            if latitude.is_finite()
                && longitude.is_finite()
                && (-90.0..=90.0).contains(&latitude)
                && (-180.0..=180.0).contains(&longitude) =>
        {
            (Some(latitude), Some(longitude))
        }
        None => (None, None),
        _ => return Err(ClientError::new(ClientErrorClass::Protocol)),
    };
    let request = UpdateAssetRequest {
        date_time_original: metadata.taken_at_utc.as_deref(),
        description: metadata.description.as_deref(),
        latitude,
        longitude,
    };
    let valid = request.date_time_original.is_some()
        || request.description.is_some()
        || request.latitude.is_some();
    valid
        .then_some(request)
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
}

fn valid_timestamp(value: &str) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .and_then(|instant| instant.format(&Rfc3339).ok())
        .is_some_and(|canonical| canonical == value && value.ends_with('Z'))
}

fn validate_album_name(name: &str) -> Result<(), ClientError> {
    let valid = !name.is_empty()
        && name.len() <= 4_096
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control);
    valid
        .then_some(())
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
}

fn validate_ids(album_id: &str, asset_ids: &[&str]) -> Result<(), ClientError> {
    let valid = valid_remote_id(album_id)
        && !asset_ids.is_empty()
        && asset_ids.len() <= MAX_ALBUM_ASSETS
        && asset_ids.iter().all(|id| valid_remote_id(id))
        && asset_ids.windows(2).all(|ids| ids[0] < ids[1]);
    valid
        .then_some(())
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
}

fn validate_membership_results(
    expected: &[&str],
    results: Vec<BulkIdResponse>,
) -> Result<(), ClientError> {
    let actual = results
        .into_iter()
        .map(|result| {
            let converged = result.success || result.error == Some(BulkIdError::Duplicate);
            (result.id, converged)
        })
        .collect::<BTreeMap<_, _>>();
    let valid = actual.len() == expected.len()
        && expected
            .iter()
            .all(|id| actual.get(*id).is_some_and(|success| *success));
    valid
        .then_some(())
        .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
}

fn valid_remote_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == MAX_REMOTE_ID_BYTES
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[14] == b'4'
        && bytes[18] == b'-'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
        && bytes[23] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ClientError> {
    (!cancellation.is_cancelled())
        .then_some(())
        .ok_or_else(|| ClientError::new(ClientErrorClass::Cancelled))
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
