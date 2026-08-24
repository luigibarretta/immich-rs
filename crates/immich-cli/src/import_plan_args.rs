use std::ffi::OsString;

use immich_rs_executor::{ApplePhotosImportConfig, TakeoutImportConfig};

use crate::args::{self, UploadApplePhotosRequest, UploadTakeoutRequest};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;
use crate::plan_args;

pub fn parse_takeout_upload(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<UploadTakeoutRequest, CliFailure> {
    let mut server = effective.server.clone();
    let mut production_read = false;
    let mut ca_certificate = effective.ca_certificate.clone();
    let mut source_arguments = Vec::with_capacity(arguments.len());
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
    let source = plan_args::parse_google_takeout(&source_arguments, effective)?;
    let mut upload = args::upload_config(effective);
    upload.scan.clone_from(&source.config.scan);
    Ok(UploadTakeoutRequest {
        inputs: source.inputs,
        label: source.label,
        config: TakeoutImportConfig {
            source: source.config,
            upload,
        },
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
        production_read,
        ca_certificate,
    })
}

pub fn parse_apple_photos_upload(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<UploadApplePhotosRequest, CliFailure> {
    let mut server = effective.server.clone();
    let mut production_read = false;
    let mut ca_certificate = effective.ca_certificate.clone();
    let mut source_arguments = Vec::with_capacity(arguments.len());
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
    let source = plan_args::parse_apple_photos(&source_arguments, effective)?;
    let mut upload = args::upload_config(effective);
    upload.scan.clone_from(&source.config.scan);
    Ok(UploadApplePhotosRequest {
        inputs: source.inputs,
        label: source.label,
        config: ApplePhotosImportConfig {
            source: source.config,
            upload,
        },
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
        production_read,
        ca_certificate,
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::parse_takeout_upload;
    use crate::config::EffectiveConfig;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn server_flags_are_separate_from_takeout_inputs() -> Result<(), &'static str> {
        let request = parse_takeout_upload(
            &arguments(&[
                "--server",
                "https://example.invalid",
                "--authorize-production-read",
                "--label",
                "synthetic-takeout",
                "first.zip",
                "second.zip",
            ]),
            &EffectiveConfig::default(),
        )
        .map_err(|_| "Takeout upload request was rejected")?;
        assert_eq!(request.inputs.len(), 2);
        assert_eq!(request.label, "synthetic-takeout");
        assert!(request.production_read);
        assert_eq!(request.config.source.scan, request.config.upload.scan);
        Ok(())
    }
}
