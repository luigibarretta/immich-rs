use std::ffi::OsString;
use std::path::PathBuf;

use immich_rs_application::PicasaImportConfig;

use crate::args::{self, PicasaRequest, UploadPicasaRequest};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;
use crate::plan_args::{common_scan_option, configured_inputs};

pub fn parse(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<PicasaRequest, CliFailure> {
    let mut label = effective
        .label
        .clone()
        .map_or_else(|| "picasa".to_owned(), |value| value);
    let mut config = args::picasa_config(effective)?;
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = common_scan_option(arguments, index, &mut config.scan)? {
            index += consumed;
            continue;
        }
        let consumed = match arguments[index].to_str() {
            Some("--label") => {
                label = args::string_value(arguments, index, "--label")?;
                2
            }
            Some("--max-archives") => {
                config.max_archives = args::usize_value(arguments, index, "--max-archives")?;
                2
            }
            Some("--max-archive-entry-bytes") => {
                config.max_archive_entry_bytes =
                    args::u64_value(arguments, index, "--max-archive-entry-bytes")?;
                2
            }
            Some("--max-compression-ratio") => {
                config.max_compression_ratio =
                    args::u64_value(arguments, index, "--max-compression-ratio")?;
                2
            }
            Some("--compression-ratio-grace-bytes") => {
                config.compression_ratio_grace_bytes =
                    args::u64_value(arguments, index, "--compression-ratio-grace-bytes")?;
                2
            }
            Some("--album-mode") => {
                config.album_mode =
                    args::album_mode(&args::string_value(arguments, index, "--album-mode")?)?;
                2
            }
            Some("--album-path-joiner") => {
                config.album_path_joiner =
                    args::string_value(arguments, index, "--album-path-joiner")?;
                2
            }
            Some("--picasa-albums") => {
                config.picasa_albums = true;
                1
            }
            Some("--no-picasa-albums") => {
                config.picasa_albums = false;
                1
            }
            Some("--filename-date") => {
                config.filename_date = true;
                1
            }
            Some("--no-filename-date") => {
                config.filename_date = false;
                1
            }
            Some(value) if value.starts_with('-') => {
                return Err(CliFailure::usage("unsupported Picasa plan option"));
            }
            _ => {
                inputs.push(PathBuf::from(&arguments[index]));
                1
            }
        };
        index += consumed;
    }
    Ok(PicasaRequest {
        inputs: configured_inputs(inputs, effective)?,
        label,
        config,
    })
}

pub fn parse_upload(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<UploadPicasaRequest, CliFailure> {
    let mut server = effective.server.clone();
    let mut production_read = false;
    let mut ca_certificate = effective.ca_certificate.clone();
    let mut source_arguments = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--server") => {
                server = Some(args::string_value(arguments, index, "--server")?);
                index += 2;
            }
            Some("--authorize-production-read") => {
                production_read = true;
                index += 1;
            }
            Some("--ca-certificate") => {
                ca_certificate = Some(args::path_value(arguments, index, "--ca-certificate")?);
                index += 2;
            }
            _ => {
                source_arguments.push(arguments[index].clone());
                index += 1;
            }
        }
    }
    let source = parse(&source_arguments, effective)?;
    let mut upload = args::upload_config(effective);
    upload.scan.clone_from(&source.config.scan);
    Ok(UploadPicasaRequest {
        inputs: source.inputs,
        label: source.label,
        config: PicasaImportConfig {
            source: source.config,
            upload,
        },
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
        production_read,
        ca_certificate,
    })
}
