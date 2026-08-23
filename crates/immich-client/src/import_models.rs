use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssetRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_time_original: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

#[derive(Deserialize)]
pub struct AssetResponse {
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumResponse {
    pub id: String,
    pub album_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAlbumRequest<'a> {
    pub album_name: &'a str,
}

#[derive(Serialize)]
pub struct BulkIdsRequest<'a> {
    pub ids: &'a [&'a str],
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BulkIdError {
    Duplicate,
    NoPermission,
    NotFound,
    Unknown,
    Validation,
}

#[derive(Deserialize)]
pub struct BulkIdResponse {
    pub id: String,
    pub success: bool,
    pub error: Option<BulkIdError>,
}
