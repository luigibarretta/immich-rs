use std::ffi::OsString;
use std::path::PathBuf;

use immich_rs_sources::FolderScanConfig;

use crate::args::{
    self, ApplePhotosRequest, ArchivePlanRequest, FolderRequest, TakeoutRequest,
    UploadFolderRequest,
};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;

pub fn parse_folder(
    arguments: &[OsString],
    config: &EffectiveConfig,
) -> Result<FolderRequest, CliFailure> {
    parse_folder_options(arguments, config, false, "folder").map(|(request, _, _, _)| request)
}

pub fn parse_google_takeout(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<TakeoutRequest, CliFailure> {
    let mut label = effective
        .label
        .clone()
        .unwrap_or_else(|| "google-takeout".to_owned());
    let mut config = args::takeout_config(effective);
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = common_scan_option(arguments, index, &mut config.scan)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--label") => label = args::string_value(arguments, index, "--label")?,
            Some("--max-archives") => {
                config.max_archives = args::usize_value(arguments, index, "--max-archives")?;
            }
            Some("--max-archive-entry-bytes") => {
                config.max_archive_entry_bytes =
                    args::u64_value(arguments, index, "--max-archive-entry-bytes")?;
            }
            Some("--max-compression-ratio") => {
                config.max_compression_ratio =
                    args::u64_value(arguments, index, "--max-compression-ratio")?;
            }
            Some("--compression-ratio-grace-bytes") => {
                config.compression_ratio_grace_bytes =
                    args::u64_value(arguments, index, "--compression-ratio-grace-bytes")?;
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
    let inputs = configured_inputs(inputs, effective)?;
    Ok(TakeoutRequest {
        inputs,
        label,
        config,
    })
}

pub fn parse_apple_photos(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<ApplePhotosRequest, CliFailure> {
    let mut label = effective
        .label
        .clone()
        .unwrap_or_else(|| "apple-photos".to_owned());
    let mut config = args::apple_config(effective)?;
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = common_scan_option(arguments, index, &mut config.scan)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--label") => label = args::string_value(arguments, index, "--label")?,
            Some("--max-archives") => {
                config.max_archives = args::usize_value(arguments, index, "--max-archives")?;
            }
            Some("--max-archive-entry-bytes") => {
                config.max_archive_entry_bytes =
                    args::u64_value(arguments, index, "--max-archive-entry-bytes")?;
            }
            Some("--max-compression-ratio") => {
                config.max_compression_ratio =
                    args::u64_value(arguments, index, "--max-compression-ratio")?;
            }
            Some("--compression-ratio-grace-bytes") => {
                config.compression_ratio_grace_bytes =
                    args::u64_value(arguments, index, "--compression-ratio-grace-bytes")?;
            }
            Some("--album-mode") => {
                config.album_mode =
                    args::album_mode(&args::string_value(arguments, index, "--album-mode")?)?;
            }
            Some("--album-path-joiner") => {
                config.album_path_joiner =
                    args::string_value(arguments, index, "--album-path-joiner")?;
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
    let inputs = configured_inputs(inputs, effective)?;
    Ok(ApplePhotosRequest {
        inputs,
        label,
        config,
    })
}

pub fn parse_upload_folder(
    arguments: &[OsString],
    config: &EffectiveConfig,
) -> Result<UploadFolderRequest, CliFailure> {
    let (folder, server, production_read, ca_certificate) =
        parse_folder_options(arguments, config, true, "folder")?;
    Ok(UploadFolderRequest {
        folder,
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
        production_read,
        ca_certificate,
    })
}

fn parse_folder_options(
    arguments: &[OsString],
    effective: &EffectiveConfig,
    allow_server: bool,
    default_label: &str,
) -> Result<(FolderRequest, Option<String>, bool, Option<PathBuf>), CliFailure> {
    let mut label = effective
        .label
        .clone()
        .unwrap_or_else(|| default_label.to_owned());
    let mut config = args::folder_config(effective);
    let mut root = None;
    let mut server = effective.server.clone();
    let mut production_read = false;
    let mut ca_certificate = effective.ca_certificate.clone();
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = common_scan_option(arguments, index, &mut config)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--label") => label = args::string_value(arguments, index, "--label")?,
            Some("--server") if allow_server => {
                server = Some(args::string_value(arguments, index, "--server")?);
            }
            Some("--authorize-production-read") if allow_server => {
                production_read = true;
                index += 1;
                continue;
            }
            Some("--ca-certificate") if allow_server => {
                ca_certificate = Some(args::path_value(arguments, index, "--ca-certificate")?);
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
            root: root
                .or_else(|| effective.source.clone())
                .ok_or_else(|| CliFailure::usage("source path is required"))?,
            label,
            config,
        },
        server,
        production_read,
        ca_certificate,
    ))
}

pub fn parse_archive_plan(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<ArchivePlanRequest, CliFailure> {
    let mut server = effective.server.clone();
    let mut config = args::archive_config(effective)?;
    let mut production_read = false;
    let mut ca_certificate = effective.ca_certificate.clone();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--server") => {
                server = Some(args::string_value(arguments, index, "--server")?);
            }
            Some("--authorize-production-read") => {
                production_read = true;
                index += 1;
                continue;
            }
            Some("--ca-certificate") => {
                ca_certificate = Some(args::path_value(arguments, index, "--ca-certificate")?);
            }
            Some("--selection") => {
                let value = args::string_value(arguments, index, "--selection")?;
                config.selection = args::archive_selection(&value)?;
            }
            Some("--include-trashed") => {
                config.include_trashed = true;
                index += 1;
                continue;
            }
            Some("--exclude-trashed") => {
                config.include_trashed = false;
                index += 1;
                continue;
            }
            Some("--page-size") => {
                config.page_size = args::usize_value(arguments, index, "--page-size")?;
            }
            Some("--max-assets") => {
                config.max_assets = args::usize_value(arguments, index, "--max-assets")?;
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
        production_read,
        ca_certificate,
    })
}

fn configured_inputs(
    cli_inputs: Vec<PathBuf>,
    config: &EffectiveConfig,
) -> Result<Vec<PathBuf>, CliFailure> {
    let inputs = if cli_inputs.is_empty() {
        config
            .inputs
            .clone()
            .or_else(|| config.source.clone().map(|source| vec![source]))
            .unwrap_or_default()
    } else {
        cli_inputs
    };
    if inputs.is_empty() {
        Err(CliFailure::usage("at least one input is required"))
    } else {
        Ok(inputs)
    }
}

pub fn common_scan_option(
    arguments: &[OsString],
    index: usize,
    config: &mut FolderScanConfig,
) -> Result<Option<usize>, CliFailure> {
    let consumed = match arguments[index].to_str() {
        Some("--buffer-bytes") => {
            config.buffer_bytes = args::usize_value(arguments, index, "--buffer-bytes")?;
            2
        }
        Some("--max-entries") => {
            config.max_entries = args::usize_value(arguments, index, "--max-entries")?;
            2
        }
        Some("--max-directory-entries") => {
            config.max_directory_entries =
                args::usize_value(arguments, index, "--max-directory-entries")?;
            2
        }
        Some("--max-path-bytes") => {
            config.max_path_bytes = args::usize_value(arguments, index, "--max-path-bytes")?;
            2
        }
        Some("--case-sensitive") => {
            config.case_sensitive = true;
            1
        }
        Some("--case-insensitive") => {
            config.case_sensitive = false;
            1
        }
        _ => return Ok(None),
    };
    Ok(Some(consumed))
}

#[cfg(test)]
mod tests {
    use super::{parse_archive_plan, parse_folder, parse_upload_folder};
    use crate::config::EffectiveConfig;
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn production_read_is_an_explicit_server_command_flag() {
        let config = EffectiveConfig::default();
        let upload = parse_upload_folder(
            &arguments(&[
                "--server",
                "https://example.invalid",
                "--authorize-production-read",
                "synthetic-source",
            ]),
            &config,
        );
        assert!(matches!(upload, Ok(request) if request.production_read));

        let archive = parse_archive_plan(
            &arguments(&[
                "--server",
                "https://example.invalid",
                "--authorize-production-read",
            ]),
            &config,
        );
        assert!(matches!(archive, Ok(request) if request.production_read));

        let folder = parse_folder(
            &arguments(&["--authorize-production-read", "synthetic-source"]),
            &config,
        );
        assert!(folder.is_err());
    }
}
