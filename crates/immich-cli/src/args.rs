use std::ffi::OsString;
use std::path::PathBuf;

use immich_rs_sources::FolderScanConfig;

use crate::failure::CliFailure;

pub struct FolderRequest {
    pub root: PathBuf,
    pub label: String,
    pub config: FolderScanConfig,
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
}

pub fn parse_folder(arguments: &[OsString]) -> Result<FolderRequest, CliFailure> {
    parse_folder_options(arguments, false).map(|(request, _)| request)
}

pub fn parse_upload_folder(arguments: &[OsString]) -> Result<UploadFolderRequest, CliFailure> {
    let (folder, server) = parse_folder_options(arguments, true)?;
    Ok(UploadFolderRequest {
        folder,
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
    })
}

fn parse_folder_options(
    arguments: &[OsString],
    allow_server: bool,
) -> Result<(FolderRequest, Option<String>), CliFailure> {
    let mut label = "folder".to_owned();
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
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--plan") => plan = Some(path_value(arguments, index, "--plan")?),
            Some("--source") => source = Some(path_value(arguments, index, "--source")?),
            Some("--checkpoint") => {
                checkpoint = Some(path_value(arguments, index, "--checkpoint")?);
            }
            Some("--server") => server = Some(string_value(arguments, index, "--server")?),
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
