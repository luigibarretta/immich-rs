use std::ffi::OsString;
use std::path::PathBuf;

use immich_rs_executor::{ArchivePlanningConfig, ArchiveSelection, UploadExecutionConfig};
use immich_rs_sources::{AlbumMode, ApplePhotosScanConfig, FolderScanConfig, TakeoutScanConfig};

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
}

pub struct ApplyRequest {
    pub plan: PathBuf,
    pub source: PathBuf,
    pub checkpoint: PathBuf,
    pub server: Option<String>,
    pub dry_run: bool,
    pub config: UploadExecutionConfig,
}

pub struct ArchivePlanRequest {
    pub server: String,
    pub config: ArchivePlanningConfig,
}

pub struct ArchiveApplyRequest {
    pub manifest: PathBuf,
    pub destination: PathBuf,
    pub server: String,
}

pub fn parse_folder(arguments: &[OsString]) -> Result<FolderRequest, CliFailure> {
    parse_folder_options(arguments, false, "folder").map(|(request, _)| request)
}

pub fn parse_google_takeout(arguments: &[OsString]) -> Result<TakeoutRequest, CliFailure> {
    let mut label = "google-takeout".to_owned();
    let mut config = TakeoutScanConfig::default();
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--label") => label = string_value(arguments, index, "--label")?,
            Some("--buffer-bytes") => {
                config.scan.buffer_bytes = usize_value(arguments, index, "--buffer-bytes")?;
            }
            Some("--max-entries") => {
                config.scan.max_entries = usize_value(arguments, index, "--max-entries")?;
            }
            Some("--max-directory-entries") => {
                config.scan.max_directory_entries =
                    usize_value(arguments, index, "--max-directory-entries")?;
            }
            Some(value) if value.starts_with('-') => {
                return Err(CliFailure::usage("unsupported Takeout plan option"));
            }
            _ => {
                inputs.push(PathBuf::from(&arguments[index]));
                index += 1;
                continue;
            }
        }
        index += 2;
    }
    if inputs.is_empty() {
        return Err(CliFailure::usage("at least one Takeout input is required"));
    }
    Ok(TakeoutRequest {
        inputs,
        label,
        config,
    })
}

pub fn parse_apple_photos(arguments: &[OsString]) -> Result<ApplePhotosRequest, CliFailure> {
    let mut label = "apple-photos".to_owned();
    let mut config = ApplePhotosScanConfig::default();
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--label") => label = string_value(arguments, index, "--label")?,
            Some("--buffer-bytes") => {
                config.scan.buffer_bytes = usize_value(arguments, index, "--buffer-bytes")?;
            }
            Some("--max-entries") => {
                config.scan.max_entries = usize_value(arguments, index, "--max-entries")?;
            }
            Some("--max-directory-entries") => {
                config.scan.max_directory_entries =
                    usize_value(arguments, index, "--max-directory-entries")?;
            }
            Some("--album-mode") => {
                config.album_mode = match string_value(arguments, index, "--album-mode")?.as_str() {
                    "none" => AlbumMode::None,
                    "folder" => AlbumMode::Folder,
                    "path" => AlbumMode::Path,
                    _ => {
                        return Err(CliFailure::usage(
                            "--album-mode requires none, folder or path",
                        ));
                    }
                };
            }
            Some("--album-path-joiner") => {
                config.album_path_joiner = string_value(arguments, index, "--album-path-joiner")?;
            }
            Some(value) if value.starts_with('-') => {
                return Err(CliFailure::usage("unsupported Apple Photos plan option"));
            }
            _ => {
                inputs.push(PathBuf::from(&arguments[index]));
                index += 1;
                continue;
            }
        }
        index += 2;
    }
    if inputs.is_empty() {
        return Err(CliFailure::usage(
            "at least one Apple Photos input is required",
        ));
    }
    Ok(ApplePhotosRequest {
        inputs,
        label,
        config,
    })
}

pub fn parse_upload_folder(arguments: &[OsString]) -> Result<UploadFolderRequest, CliFailure> {
    let (folder, server) = parse_folder_options(arguments, true, "folder")?;
    Ok(UploadFolderRequest {
        folder,
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
    })
}

fn parse_folder_options(
    arguments: &[OsString],
    allow_server: bool,
    default_label: &str,
) -> Result<(FolderRequest, Option<String>), CliFailure> {
    let mut label = default_label.to_owned();
    let mut config = FolderScanConfig::default();
    let mut root = None;
    let mut server = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--label") => label = string_value(arguments, index, "--label")?,
            Some("--buffer-bytes") => {
                config.buffer_bytes = usize_value(arguments, index, "--buffer-bytes")?;
            }
            Some("--max-entries") => {
                config.max_entries = usize_value(arguments, index, "--max-entries")?;
            }
            Some("--max-directory-entries") => {
                config.max_directory_entries =
                    usize_value(arguments, index, "--max-directory-entries")?;
            }
            Some("--server") if allow_server => {
                server = Some(string_value(arguments, index, "--server")?);
            }
            Some(value) if value.starts_with('-') => {
                return Err(CliFailure::usage("unsupported plan option"));
            }
            _ if root.is_none() => {
                root = Some(PathBuf::from(&arguments[index]));
                index += 1;
                continue;
            }
            _ => return Err(CliFailure::usage("plan accepts exactly one source path")),
        }
        index += 2;
    }
    Ok((
        FolderRequest {
            root: root.ok_or_else(|| CliFailure::usage("source path is required"))?,
            label,
            config,
        },
        server,
    ))
}

pub fn parse_apply(arguments: &[OsString]) -> Result<ApplyRequest, CliFailure> {
    let mut plan = None;
    let mut source = None;
    let mut checkpoint = None;
    let mut server = None;
    let mut dry_run = false;
    let mut config = UploadExecutionConfig::default();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--plan") => plan = Some(path_value(arguments, index, "--plan")?),
            Some("--source") => source = Some(path_value(arguments, index, "--source")?),
            Some("--checkpoint") => {
                checkpoint = Some(path_value(arguments, index, "--checkpoint")?);
            }
            Some("--server") => server = Some(string_value(arguments, index, "--server")?),
            Some("--buffer-bytes") => {
                config.scan.buffer_bytes = usize_value(arguments, index, "--buffer-bytes")?;
            }
            Some("--max-entries") => {
                config.scan.max_entries = usize_value(arguments, index, "--max-entries")?;
            }
            Some("--max-directory-entries") => {
                config.scan.max_directory_entries =
                    usize_value(arguments, index, "--max-directory-entries")?;
            }
            Some("--dry-run") if !dry_run => {
                dry_run = true;
                index += 1;
                continue;
            }
            _ => return Err(CliFailure::usage("unsupported apply option")),
        }
        index += 2;
    }
    if dry_run && server.is_some() {
        return Err(CliFailure::usage("dry-run does not accept --server"));
    }
    if !dry_run && server.is_none() {
        return Err(CliFailure::usage("--server is required for apply"));
    }
    Ok(ApplyRequest {
        plan: plan.ok_or_else(|| CliFailure::usage("--plan is required"))?,
        source: source.ok_or_else(|| CliFailure::usage("--source is required"))?,
        checkpoint: checkpoint.ok_or_else(|| CliFailure::usage("--checkpoint is required"))?,
        server,
        dry_run,
        config,
    })
}

pub fn parse_archive_plan(arguments: &[OsString]) -> Result<ArchivePlanRequest, CliFailure> {
    let mut server = None;
    let mut config = ArchivePlanningConfig::default();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--server") => server = Some(string_value(arguments, index, "--server")?),
            Some("--selection") => {
                config.selection = match string_value(arguments, index, "--selection")?.as_str() {
                    "timeline" => ArchiveSelection::Timeline,
                    "archive" => ArchiveSelection::Archive,
                    "hidden" => ArchiveSelection::Hidden,
                    "all" => ArchiveSelection::All,
                    _ => return Err(CliFailure::usage("invalid archive selection")),
                };
            }
            Some("--include-trashed") if !config.include_trashed => {
                config.include_trashed = true;
                index += 1;
                continue;
            }
            Some("--page-size") => {
                config.page_size = usize_value(arguments, index, "--page-size")?;
            }
            Some("--max-assets") => {
                config.max_assets = usize_value(arguments, index, "--max-assets")?;
            }
            _ => return Err(CliFailure::usage("unsupported archive plan option")),
        }
        index += 2;
    }
    config
        .validate()
        .map_err(|_| CliFailure::usage("invalid archive resource limits"))?;
    Ok(ArchivePlanRequest {
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
        config,
    })
}

pub fn parse_archive_apply(arguments: &[OsString]) -> Result<ArchiveApplyRequest, CliFailure> {
    let mut manifest = None;
    let mut destination = None;
    let mut server = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--manifest") => {
                manifest = Some(path_value(arguments, index, "--manifest")?);
            }
            Some("--destination") => {
                destination = Some(path_value(arguments, index, "--destination")?);
            }
            Some("--server") => server = Some(string_value(arguments, index, "--server")?),
            _ => return Err(CliFailure::usage("unsupported archive apply option")),
        }
        index += 2;
    }
    Ok(ArchiveApplyRequest {
        manifest: manifest.ok_or_else(|| CliFailure::usage("--manifest is required"))?,
        destination: destination.ok_or_else(|| CliFailure::usage("--destination is required"))?,
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
    })
}

fn string_value(arguments: &[OsString], index: usize, option: &str) -> Result<String, CliFailure> {
    arguments
        .get(index + 1)
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| CliFailure::usage_owned(format!("{option} requires a Unicode value")))
}

fn usize_value(arguments: &[OsString], index: usize, option: &str) -> Result<usize, CliFailure> {
    string_value(arguments, index, option)?
        .parse::<usize>()
        .map_err(|_| CliFailure::usage_owned(format!("{option} requires a positive integer")))
}

fn path_value(arguments: &[OsString], index: usize, option: &str) -> Result<PathBuf, CliFailure> {
    arguments
        .get(index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| CliFailure::usage_owned(format!("{option} requires a path")))
}
