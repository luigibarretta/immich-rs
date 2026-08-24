use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use immich_rs_executor::{
    ApplePhotosImportConfig, ArchivePlanningConfig, ArchiveSelection, TakeoutImportConfig,
    UploadExecutionConfig,
};
use immich_rs_sources::{AlbumMode, ApplePhotosScanConfig, FolderScanConfig, TakeoutScanConfig};

use crate::config::EffectiveConfig;
use crate::failure::CliFailure;

pub struct FolderRequest {
    pub root: PathBuf,
    pub label: String,
    pub config: FolderScanConfig,
}

pub struct TakeoutRequest {
    pub inputs: Vec<PathBuf>,
    pub label: String,
    pub config: TakeoutScanConfig,
}

pub struct ApplePhotosRequest {
    pub inputs: Vec<PathBuf>,
    pub label: String,
    pub config: ApplePhotosScanConfig,
}

pub struct UploadFolderRequest {
    pub folder: FolderRequest,
    pub server: String,
    pub production_read: bool,
    pub ca_certificate: Option<PathBuf>,
}

pub struct UploadTakeoutRequest {
    pub inputs: Vec<PathBuf>,
    pub label: String,
    pub config: TakeoutImportConfig,
    pub server: String,
    pub production_read: bool,
    pub ca_certificate: Option<PathBuf>,
}

pub struct UploadApplePhotosRequest {
    pub inputs: Vec<PathBuf>,
    pub label: String,
    pub config: ApplePhotosImportConfig,
    pub server: String,
    pub production_read: bool,
    pub ca_certificate: Option<PathBuf>,
}

pub struct ApplyRequest {
    pub plan: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub checkpoint: PathBuf,
    pub server: Option<String>,
    pub dry_run: bool,
    pub config: UploadExecutionConfig,
    pub takeout: TakeoutScanConfig,
    pub apple: ApplePhotosScanConfig,
    pub production: Option<ProductionWriteRequest>,
    pub ca_certificate: Option<PathBuf>,
}

pub struct ProductionWriteRequest {
    pub plan_sha256: String,
    pub expected_operations: u64,
    pub backup_reference: String,
}

pub struct ArchivePlanRequest {
    pub server: String,
    pub config: ArchivePlanningConfig,
    pub production_read: bool,
    pub ca_certificate: Option<PathBuf>,
}

pub struct ArchiveApplyRequest {
    pub manifest: PathBuf,
    pub destination: PathBuf,
    pub server: String,
    pub production_read: bool,
    pub ca_certificate: Option<PathBuf>,
}

pub fn folder_config(config: &EffectiveConfig) -> FolderScanConfig {
    let mut scan = FolderScanConfig::default();
    scan.buffer_bytes = config.buffer_bytes.unwrap_or(scan.buffer_bytes);
    scan.max_entries = config.max_entries.unwrap_or(scan.max_entries);
    scan.max_directory_entries = config
        .max_directory_entries
        .unwrap_or(scan.max_directory_entries);
    scan.max_path_bytes = config.max_path_bytes.unwrap_or(scan.max_path_bytes);
    scan.case_sensitive = config.case_sensitive.unwrap_or(scan.case_sensitive);
    scan
}

pub fn takeout_config(config: &EffectiveConfig) -> TakeoutScanConfig {
    let mut scan = TakeoutScanConfig {
        scan: folder_config(config),
        ..TakeoutScanConfig::default()
    };
    scan.max_archives = config.max_archives.unwrap_or(scan.max_archives);
    scan.max_archive_entry_bytes = config
        .max_archive_entry_bytes
        .unwrap_or(scan.max_archive_entry_bytes);
    scan.max_compression_ratio = config
        .max_compression_ratio
        .unwrap_or(scan.max_compression_ratio);
    scan.compression_ratio_grace_bytes = config
        .compression_ratio_grace_bytes
        .unwrap_or(scan.compression_ratio_grace_bytes);
    scan
}

pub fn apple_config(config: &EffectiveConfig) -> Result<ApplePhotosScanConfig, CliFailure> {
    let mut scan = ApplePhotosScanConfig {
        scan: folder_config(config),
        ..ApplePhotosScanConfig::default()
    };
    scan.max_archives = config.max_archives.unwrap_or(scan.max_archives);
    scan.max_archive_entry_bytes = config
        .max_archive_entry_bytes
        .unwrap_or(scan.max_archive_entry_bytes);
    scan.max_compression_ratio = config
        .max_compression_ratio
        .unwrap_or(scan.max_compression_ratio);
    scan.compression_ratio_grace_bytes = config
        .compression_ratio_grace_bytes
        .unwrap_or(scan.compression_ratio_grace_bytes);
    if let Some(value) = config.album_mode.as_deref() {
        scan.album_mode = album_mode(value)?;
    }
    if let Some(value) = &config.album_path_joiner {
        scan.album_path_joiner.clone_from(value);
    }
    Ok(scan)
}

pub fn upload_config(config: &EffectiveConfig) -> UploadExecutionConfig {
    let mut upload = UploadExecutionConfig {
        scan: folder_config(config),
        ..UploadExecutionConfig::default()
    };
    upload.verification_buffer_bytes = config
        .verification_buffer_bytes
        .unwrap_or(upload.verification_buffer_bytes);
    upload.concurrency = config.concurrency.unwrap_or(upload.concurrency);
    upload.max_attempts_per_operation = config
        .max_attempts_per_operation
        .unwrap_or(upload.max_attempts_per_operation);
    upload.max_retries_per_run = config
        .max_retries_per_run
        .unwrap_or(upload.max_retries_per_run);
    upload.retry_base_delay = config
        .retry_base_delay_ms
        .map_or(upload.retry_base_delay, Duration::from_millis);
    upload.retry_delay_cap = config
        .retry_delay_cap_ms
        .map_or(upload.retry_delay_cap, Duration::from_millis);
    upload
}

pub fn archive_config(config: &EffectiveConfig) -> Result<ArchivePlanningConfig, CliFailure> {
    let mut archive = ArchivePlanningConfig::default();
    if let Some(value) = config.archive_selection.as_deref() {
        archive.selection = archive_selection(value)?;
    }
    archive.include_trashed = config
        .archive_include_trashed
        .unwrap_or(archive.include_trashed);
    archive.page_size = config.archive_page_size.unwrap_or(archive.page_size);
    archive.max_assets = config.archive_max_assets.unwrap_or(archive.max_assets);
    Ok(archive)
}

pub fn album_mode(value: &str) -> Result<AlbumMode, CliFailure> {
    match value {
        "none" => Ok(AlbumMode::None),
        "folder" => Ok(AlbumMode::Folder),
        "path" => Ok(AlbumMode::Path),
        _ => Err(CliFailure::usage(
            "--album-mode requires none, folder or path",
        )),
    }
}

pub fn archive_selection(value: &str) -> Result<ArchiveSelection, CliFailure> {
    match value {
        "timeline" => Ok(ArchiveSelection::Timeline),
        "archive" => Ok(ArchiveSelection::Archive),
        "hidden" => Ok(ArchiveSelection::Hidden),
        "all" => Ok(ArchiveSelection::All),
        _ => Err(CliFailure::usage("invalid archive selection")),
    }
}

pub fn string_value(
    arguments: &[OsString],
    index: usize,
    option: &str,
) -> Result<String, CliFailure> {
    arguments
        .get(index + 1)
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| CliFailure::usage_owned(format!("{option} requires a Unicode value")))
}

pub fn usize_value(
    arguments: &[OsString],
    index: usize,
    option: &str,
) -> Result<usize, CliFailure> {
    parse_number(arguments, index, option)
}

pub fn u64_value(arguments: &[OsString], index: usize, option: &str) -> Result<u64, CliFailure> {
    parse_number(arguments, index, option)
}

pub fn u32_value(arguments: &[OsString], index: usize, option: &str) -> Result<u32, CliFailure> {
    parse_number(arguments, index, option)
}

fn parse_number<T>(arguments: &[OsString], index: usize, option: &str) -> Result<T, CliFailure>
where
    T: std::str::FromStr,
{
    string_value(arguments, index, option)?
        .parse()
        .map_err(|_| CliFailure::usage_owned(format!("{option} requires a positive integer")))
}

pub fn path_value(
    arguments: &[OsString],
    index: usize,
    option: &str,
) -> Result<PathBuf, CliFailure> {
    arguments
        .get(index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| CliFailure::usage_owned(format!("{option} requires a path")))
}
