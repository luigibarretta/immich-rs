use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionResponse {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<u32>,
}

#[derive(Deserialize)]
pub struct UserResponse {
    pub id: String,
}

#[derive(Serialize)]
pub struct BulkCheckRequest<'a> {
    pub assets: [BulkCheckAsset<'a>; 1],
}

#[derive(Serialize)]
pub struct BulkCheckAsset<'a> {
    pub id: &'a str,
    pub checksum: &'a str,
}

#[derive(Deserialize)]
pub struct BulkCheckResponse {
    pub results: Vec<BulkCheckResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkCheckResult {
    pub id: String,
    pub action: BulkCheckAction,
    pub asset_id: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BulkCheckAction {
    Accept,
    Reject,
}

#[derive(Deserialize)]
pub struct UploadResponse {
    pub id: String,
    pub status: UploadStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSearchRequest<'a> {
    pub page: u64,
    pub size: usize,
    pub order: &'static str,
    pub with_exif: bool,
    pub visibility: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed_after: Option<&'static str>,
}

#[derive(Deserialize)]
pub struct ArchiveSearchResponse {
    pub assets: ArchiveSearchAssets,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSearchAssets {
    pub items: Vec<ArchiveAssetResponse>,
    pub count: u64,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub total: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveAssetResponse {
    pub id: String,
    pub original_file_name: String,
    pub checksum: String,
    pub r#type: AssetTypeResponse,
    pub exif_info: Option<ArchiveExifResponse>,
    #[serde(default)]
    pub live_photo_video_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveExifResponse {
    pub file_size_in_byte: Option<u64>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetTypeResponse {
    Image,
    Video,
    Audio,
    Other,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UploadStatus {
    Created,
    Duplicate,
}
