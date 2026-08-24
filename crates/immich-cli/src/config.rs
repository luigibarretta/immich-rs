use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::environment;
use crate::failure::CliFailure;

const CONFIG_SCHEMA_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize)]
pub struct EffectiveConfig {
    pub schema_version: u32,
    pub server: Option<String>,
    pub ca_certificate: Option<PathBuf>,
    pub label: Option<String>,
    pub source: Option<PathBuf>,
    pub inputs: Option<Vec<PathBuf>>,
    pub buffer_bytes: Option<usize>,
    pub max_entries: Option<usize>,
    pub max_directory_entries: Option<usize>,
    pub max_path_bytes: Option<usize>,
    pub case_sensitive: Option<bool>,
    pub max_archives: Option<usize>,
    pub max_archive_entry_bytes: Option<u64>,
    pub max_compression_ratio: Option<u64>,
    pub compression_ratio_grace_bytes: Option<u64>,
    pub album_mode: Option<String>,
    pub album_path_joiner: Option<String>,
    pub picasa_albums: Option<bool>,
    pub picasa_filename_date: Option<bool>,
    pub upload_plan: Option<PathBuf>,
    pub upload_source: Option<PathBuf>,
    pub upload_checkpoint: Option<PathBuf>,
    pub upload_dry_run: Option<bool>,
    pub verification_buffer_bytes: Option<usize>,
    pub concurrency: Option<usize>,
    pub max_attempts_per_operation: Option<u32>,
    pub max_retries_per_run: Option<u32>,
    pub retry_base_delay_ms: Option<u64>,
    pub retry_delay_cap_ms: Option<u64>,
    pub archive_selection: Option<String>,
    pub archive_include_trashed: Option<bool>,
    pub archive_page_size: Option<usize>,
    pub archive_max_assets: Option<usize>,
    pub archive_manifest: Option<PathBuf>,
    pub archive_destination: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    schema_version: u32,
    #[serde(default)]
    immich: ImmichConfig,
    #[serde(default)]
    scan: ScanConfig,
    #[serde(default)]
    apple_photos: ApplePhotosConfig,
    #[serde(default)]
    picasa: PicasaConfig,
    #[serde(default)]
    upload: UploadConfig,
    #[serde(default)]
    archive: ArchiveConfig,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImmichConfig {
    server: Option<String>,
    ca_certificate: Option<PathBuf>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanConfig {
    label: Option<String>,
    source: Option<PathBuf>,
    inputs: Option<Vec<PathBuf>>,
    buffer_bytes: Option<usize>,
    max_entries: Option<usize>,
    max_directory_entries: Option<usize>,
    max_path_bytes: Option<usize>,
    case_sensitive: Option<bool>,
    max_archives: Option<usize>,
    max_archive_entry_bytes: Option<u64>,
    max_compression_ratio: Option<u64>,
    compression_ratio_grace_bytes: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplePhotosConfig {
    album_mode: Option<String>,
    album_path_joiner: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PicasaConfig {
    albums: Option<bool>,
    filename_date: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadConfig {
    plan: Option<PathBuf>,
    source: Option<PathBuf>,
    checkpoint: Option<PathBuf>,
    dry_run: Option<bool>,
    verification_buffer_bytes: Option<usize>,
    concurrency: Option<usize>,
    max_attempts_per_operation: Option<u32>,
    max_retries_per_run: Option<u32>,
    retry_base_delay_ms: Option<u64>,
    retry_delay_cap_ms: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveConfig {
    selection: Option<String>,
    include_trashed: Option<bool>,
    page_size: Option<usize>,
    max_assets: Option<usize>,
    manifest: Option<PathBuf>,
    destination: Option<PathBuf>,
}

pub fn load(arguments: &[OsString]) -> Result<(EffectiveConfig, Vec<OsString>), CliFailure> {
    let (cli_path, filtered) = extract_config_argument(arguments)?;
    environment::validate_names()?;
    let environment_path = environment::config_path()?;
    let path = cli_path.or(environment_path);
    let mut config = match path {
        Some(path) => load_file(&path)?,
        None => EffectiveConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            ..EffectiveConfig::default()
        },
    };
    environment::overlay(&mut config)?;
    Ok((config, filtered))
}

fn extract_config_argument(
    arguments: &[OsString],
) -> Result<(Option<PathBuf>, Vec<OsString>), CliFailure> {
    let mut path = None;
    let mut filtered = Vec::with_capacity(arguments.len());
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--config" {
            if path.is_some() {
                return Err(CliFailure::usage("--config may be supplied only once"));
            }
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| CliFailure::usage("--config requires a path"))?;
            path = Some(PathBuf::from(value));
            index += 2;
        } else {
            filtered.push(arguments[index].clone());
            index += 1;
        }
    }
    Ok((path, filtered))
}

fn load_file(path: &Path) -> Result<EffectiveConfig, CliFailure> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| CliFailure::usage("cannot read configuration file"))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_CONFIG_BYTES
    {
        return Err(CliFailure::usage(
            "configuration must be a bounded regular file",
        ));
    }
    let source = std::fs::read_to_string(path)
        .map_err(|_| CliFailure::usage("configuration must be valid UTF-8"))?;
    let file: FileConfig = toml::from_str(&source)
        .map_err(|_| CliFailure::usage("invalid or unknown TOML configuration"))?;
    if file.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(CliFailure::usage("unsupported configuration schema"));
    }
    Ok(EffectiveConfig {
        schema_version: file.schema_version,
        server: file.immich.server,
        ca_certificate: file.immich.ca_certificate,
        label: file.scan.label,
        source: file.scan.source,
        inputs: file.scan.inputs,
        buffer_bytes: file.scan.buffer_bytes,
        max_entries: file.scan.max_entries,
        max_directory_entries: file.scan.max_directory_entries,
        max_path_bytes: file.scan.max_path_bytes,
        case_sensitive: file.scan.case_sensitive,
        max_archives: file.scan.max_archives,
        max_archive_entry_bytes: file.scan.max_archive_entry_bytes,
        max_compression_ratio: file.scan.max_compression_ratio,
        compression_ratio_grace_bytes: file.scan.compression_ratio_grace_bytes,
        album_mode: file.apple_photos.album_mode,
        album_path_joiner: file.apple_photos.album_path_joiner,
        picasa_albums: file.picasa.albums,
        picasa_filename_date: file.picasa.filename_date,
        upload_plan: file.upload.plan,
        upload_source: file.upload.source,
        upload_checkpoint: file.upload.checkpoint,
        upload_dry_run: file.upload.dry_run,
        verification_buffer_bytes: file.upload.verification_buffer_bytes,
        concurrency: file.upload.concurrency,
        max_attempts_per_operation: file.upload.max_attempts_per_operation,
        max_retries_per_run: file.upload.max_retries_per_run,
        retry_base_delay_ms: file.upload.retry_base_delay_ms,
        retry_delay_cap_ms: file.upload.retry_delay_cap_ms,
        archive_selection: file.archive.selection,
        archive_include_trashed: file.archive.include_trashed,
        archive_page_size: file.archive.page_size,
        archive_max_assets: file.archive.max_assets,
        archive_manifest: file.archive.manifest,
        archive_destination: file.archive.destination,
    })
}
