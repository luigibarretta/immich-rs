use std::ffi::OsString;
use std::time::Duration;

use immich_rs_executor::UploadExecutionConfig;
use immich_rs_sources::{ApplePhotosScanConfig, TakeoutScanConfig};

use crate::args::{self, ApplyRequest, ArchiveApplyRequest, ProductionWriteRequest};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;
use crate::plan_args::common_scan_option;

#[derive(Default)]
struct ProductionOptions {
    read: bool,
    write: bool,
    plan_sha256: Option<String>,
    expected_operations: Option<u64>,
    backup_reference: Option<String>,
}

pub fn parse_upload(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<ApplyRequest, CliFailure> {
    let mut plan = effective.upload_plan.clone();
    let mut cli_source = None;
    let mut cli_inputs = Vec::new();
    let mut checkpoint = effective.upload_checkpoint.clone();
    let mut server = effective.server.clone();
    let mut server_from_cli = false;
    let mut ca_certificate = effective.ca_certificate.clone();
    let mut ca_from_cli = false;
    let mut dry_run = effective.upload_dry_run.unwrap_or(false);
    let mut production = ProductionOptions::default();
    let mut config = args::upload_config(effective);
    let mut takeout = args::takeout_config(effective);
    let mut apple = args::apple_config(effective)?;
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = common_scan_option(arguments, index, &mut config.scan)? {
            index += consumed;
            continue;
        }
        if let Some(consumed) = production_option(arguments, index, &mut production)? {
            index += consumed;
            continue;
        }
        if let Some(consumed) = takeout_option(arguments, index, &mut takeout)? {
            index += consumed;
            continue;
        }
        if let Some(consumed) = apple_option(arguments, index, &mut apple)? {
            index += consumed;
            continue;
        }
        if let Some(consumed) = execution_option(arguments, index, &mut config)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--plan") => plan = Some(args::path_value(arguments, index, "--plan")?),
            Some("--source") => {
                if cli_source.is_some() {
                    return Err(CliFailure::usage("--source may be supplied only once"));
                }
                cli_source = Some(args::path_value(arguments, index, "--source")?);
            }
            Some("--input") => {
                cli_inputs.push(args::path_value(arguments, index, "--input")?);
            }
            Some("--checkpoint") => {
                checkpoint = Some(args::path_value(arguments, index, "--checkpoint")?);
            }
            Some("--server") => {
                server = Some(args::string_value(arguments, index, "--server")?);
                server_from_cli = true;
            }
            Some("--ca-certificate") => {
                ca_certificate = Some(args::path_value(arguments, index, "--ca-certificate")?);
                ca_from_cli = true;
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
    takeout.scan.clone_from(&config.scan);
    apple.scan.clone_from(&config.scan);
    apple.max_archives = takeout.max_archives;
    apple.max_archive_entry_bytes = takeout.max_archive_entry_bytes;
    apple.max_compression_ratio = takeout.max_compression_ratio;
    apple.compression_ratio_grace_bytes = takeout.compression_ratio_grace_bytes;
    if dry_run && (server_from_cli || ca_from_cli) {
        return Err(CliFailure::usage(
            "dry-run does not accept server transport options",
        ));
    }
    if !dry_run && server.is_none() {
        return Err(CliFailure::usage("--server is required for apply"));
    }
    let production = production.into_request(dry_run)?;
    Ok(ApplyRequest {
        plan: plan.ok_or_else(|| CliFailure::usage("--plan is required"))?,
        inputs: apply_inputs(cli_source, cli_inputs, effective)?,
        checkpoint: checkpoint.ok_or_else(|| CliFailure::usage("--checkpoint is required"))?,
        server: if dry_run { None } else { server },
        dry_run,
        config,
        takeout,
        apple,
        production,
        ca_certificate: if dry_run { None } else { ca_certificate },
    })
}

fn execution_option(
    arguments: &[OsString],
    index: usize,
    config: &mut UploadExecutionConfig,
) -> Result<Option<usize>, CliFailure> {
    match arguments[index].to_str() {
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
            config.retry_base_delay =
                Duration::from_millis(args::u64_value(arguments, index, "--retry-base-delay-ms")?);
        }
        Some("--retry-delay-cap-ms") => {
            config.retry_delay_cap =
                Duration::from_millis(args::u64_value(arguments, index, "--retry-delay-cap-ms")?);
        }
        _ => return Ok(None),
    }
    Ok(Some(2))
}

fn takeout_option(
    arguments: &[OsString],
    index: usize,
    config: &mut TakeoutScanConfig,
) -> Result<Option<usize>, CliFailure> {
    match arguments[index].to_str() {
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
        _ => return Ok(None),
    }
    Ok(Some(2))
}

fn apple_option(
    arguments: &[OsString],
    index: usize,
    config: &mut ApplePhotosScanConfig,
) -> Result<Option<usize>, CliFailure> {
    match arguments[index].to_str() {
        Some("--album-mode") => {
            config.album_mode =
                args::album_mode(&args::string_value(arguments, index, "--album-mode")?)?;
        }
        Some("--album-path-joiner") => {
            config.album_path_joiner = args::string_value(arguments, index, "--album-path-joiner")?;
        }
        _ => return Ok(None),
    }
    Ok(Some(2))
}

fn apply_inputs(
    cli_source: Option<std::path::PathBuf>,
    cli_inputs: Vec<std::path::PathBuf>,
    effective: &EffectiveConfig,
) -> Result<Vec<std::path::PathBuf>, CliFailure> {
    if cli_source.is_some() && !cli_inputs.is_empty() {
        return Err(CliFailure::usage("--source and --input cannot be mixed"));
    }
    let inputs = if !cli_inputs.is_empty() {
        cli_inputs
    } else if let Some(source) = cli_source.or_else(|| effective.upload_source.clone()) {
        vec![source]
    } else if let Some(inputs) = effective.inputs.clone() {
        inputs
    } else if let Some(source) = effective.source.clone() {
        vec![source]
    } else {
        Vec::new()
    };
    if inputs.is_empty() {
        Err(CliFailure::usage(
            "--source or at least one --input is required",
        ))
    } else {
        Ok(inputs)
    }
}

fn production_option(
    arguments: &[OsString],
    index: usize,
    production: &mut ProductionOptions,
) -> Result<Option<usize>, CliFailure> {
    let consumed = match arguments[index].to_str() {
        Some("--authorize-production-read") => {
            production.read = true;
            1
        }
        Some("--authorize-production-write") => {
            production.write = true;
            1
        }
        Some("--confirm-plan-sha256") => {
            production.plan_sha256 = Some(args::string_value(
                arguments,
                index,
                "--confirm-plan-sha256",
            )?);
            2
        }
        Some("--expected-operations") => {
            production.expected_operations =
                Some(args::u64_value(arguments, index, "--expected-operations")?);
            2
        }
        Some("--backup-reference") => {
            production.backup_reference =
                Some(args::string_value(arguments, index, "--backup-reference")?);
            2
        }
        _ => return Ok(None),
    };
    Ok(Some(consumed))
}

impl ProductionOptions {
    fn into_request(self, dry_run: bool) -> Result<Option<ProductionWriteRequest>, CliFailure> {
        let has_values = self.read
            || self.write
            || self.plan_sha256.is_some()
            || self.expected_operations.is_some()
            || self.backup_reference.is_some();
        if dry_run || !has_values {
            return Ok(None);
        }
        if !self.read || !self.write {
            return Err(CliFailure::usage(
                "production apply requires read and write acknowledgements",
            ));
        }
        Ok(Some(ProductionWriteRequest {
            plan_sha256: self.plan_sha256.ok_or_else(|| {
                CliFailure::usage("production apply requires --confirm-plan-sha256")
            })?,
            expected_operations: self.expected_operations.ok_or_else(|| {
                CliFailure::usage("production apply requires --expected-operations")
            })?,
            backup_reference: self
                .backup_reference
                .ok_or_else(|| CliFailure::usage("production apply requires --backup-reference"))?,
        }))
    }
}

pub fn parse_archive(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<ArchiveApplyRequest, CliFailure> {
    let mut manifest = effective.archive_manifest.clone();
    let mut destination = effective.archive_destination.clone();
    let mut server = effective.server.clone();
    let mut production_read = false;
    let mut ca_certificate = effective.ca_certificate.clone();
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
            Some("--ca-certificate") => {
                ca_certificate = Some(args::path_value(arguments, index, "--ca-certificate")?);
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
        ca_certificate,
    })
}
