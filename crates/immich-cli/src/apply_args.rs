use std::ffi::OsString;
use std::time::Duration;

use crate::args::{self, ApplyRequest, ArchiveApplyRequest};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;
use crate::plan_args::common_scan_option;

pub fn parse_upload(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<ApplyRequest, CliFailure> {
    let mut plan = effective.upload_plan.clone();
    let mut source = effective
        .upload_source
        .clone()
        .or_else(|| effective.source.clone());
    let mut checkpoint = effective.upload_checkpoint.clone();
    let mut server = effective.server.clone();
    let mut server_from_cli = false;
    let mut dry_run = effective.upload_dry_run.unwrap_or(false);
    let mut config = args::upload_config(effective);
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = common_scan_option(arguments, index, &mut config.scan)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--plan") => plan = Some(args::path_value(arguments, index, "--plan")?),
            Some("--source") => source = Some(args::path_value(arguments, index, "--source")?),
            Some("--checkpoint") => {
                checkpoint = Some(args::path_value(arguments, index, "--checkpoint")?);
            }
            Some("--server") => {
                server = Some(args::string_value(arguments, index, "--server")?);
                server_from_cli = true;
            }
            Some("--verification-buffer-bytes") => {
                config.verification_buffer_bytes =
                    args::usize_value(arguments, index, "--verification-buffer-bytes")?;
            }
            Some("--concurrency") => {
                config.concurrency = args::usize_value(arguments, index, "--concurrency")?;
            }
            Some("--max-attempts-per-operation") => {
                config.max_attempts_per_operation =
                    args::u32_value(arguments, index, "--max-attempts-per-operation")?;
            }
            Some("--max-retries-per-run") => {
                config.max_retries_per_run =
                    args::u32_value(arguments, index, "--max-retries-per-run")?;
            }
            Some("--retry-base-delay-ms") => {
                config.retry_base_delay = Duration::from_millis(args::u64_value(
                    arguments,
                    index,
                    "--retry-base-delay-ms",
                )?);
            }
            Some("--retry-delay-cap-ms") => {
                config.retry_delay_cap = Duration::from_millis(args::u64_value(
                    arguments,
                    index,
                    "--retry-delay-cap-ms",
                )?);
            }
            Some("--dry-run") => {
                dry_run = true;
                index += 1;
                continue;
            }
            Some("--no-dry-run") => {
                dry_run = false;
                index += 1;
                continue;
            }
            _ => return Err(CliFailure::usage("unsupported apply option")),
        }
        index += 2;
    }
    if dry_run && server_from_cli {
        return Err(CliFailure::usage("dry-run does not accept --server"));
    }
    if !dry_run && server.is_none() {
        return Err(CliFailure::usage("--server is required for apply"));
    }
    Ok(ApplyRequest {
        plan: plan.ok_or_else(|| CliFailure::usage("--plan is required"))?,
        source: source.ok_or_else(|| CliFailure::usage("--source is required"))?,
        checkpoint: checkpoint.ok_or_else(|| CliFailure::usage("--checkpoint is required"))?,
        server: if dry_run { None } else { server },
        dry_run,
        config,
    })
}

pub fn parse_archive(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<ArchiveApplyRequest, CliFailure> {
    let mut manifest = effective.archive_manifest.clone();
    let mut destination = effective.archive_destination.clone();
    let mut server = effective.server.clone();
    let mut production_read = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--manifest") => {
                manifest = Some(args::path_value(arguments, index, "--manifest")?);
            }
            Some("--destination") => {
                destination = Some(args::path_value(arguments, index, "--destination")?);
            }
            Some("--server") => {
                server = Some(args::string_value(arguments, index, "--server")?);
            }
            Some("--authorize-production-read") => {
                production_read = true;
                index += 1;
                continue;
            }
            _ => return Err(CliFailure::usage("unsupported archive apply option")),
        }
        index += 2;
    }
    Ok(ArchiveApplyRequest {
        manifest: manifest.ok_or_else(|| CliFailure::usage("--manifest is required"))?,
        destination: destination.ok_or_else(|| CliFailure::usage("--destination is required"))?,
        server: server.ok_or_else(|| CliFailure::usage("--server is required"))?,
        production_read,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_archive, parse_upload};
    use crate::config::EffectiveConfig;
    use std::ffi::OsString;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn production_read_is_limited_to_archive_apply() {
        let config = EffectiveConfig::default();
        let archive = parse_archive(
            &arguments(&[
                "--manifest",
                "manifest.json",
                "--destination",
                "archive",
                "--server",
                "https://example.invalid",
                "--authorize-production-read",
            ]),
            &config,
        );
        assert!(matches!(archive, Ok(request) if request.production_read));

        let upload = parse_upload(
            &arguments(&[
                "--plan",
                "plan.json",
                "--source",
                "source",
                "--checkpoint",
                "checkpoint.sqlite",
                "--server",
                "https://example.invalid",
                "--authorize-production-read",
            ]),
            &config,
        );
        assert!(upload.is_err());
    }
}
