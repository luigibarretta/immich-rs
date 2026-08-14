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

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UploadStatus {
    Created,
    Duplicate,
}
