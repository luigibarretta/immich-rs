use std::path::PathBuf;
use std::str::FromStr;

use crate::config::EffectiveConfig;
use crate::failure::CliFailure;

const PREFIX: &str = "IMMICH_RS_";
const KNOWN_NAMES: &[&str] = &[
    "IMMICH_RS_ALBUM_MODE",
    "IMMICH_RS_ALBUM_PATH_JOINER",
    "IMMICH_RS_API_KEY",
    "IMMICH_RS_API_KEY_FILE",
    "IMMICH_RS_ARCHIVE_DESTINATION",
    "IMMICH_RS_ARCHIVE_INCLUDE_TRASHED",
    "IMMICH_RS_ARCHIVE_MANIFEST",
    "IMMICH_RS_ARCHIVE_MAX_ASSETS",
    "IMMICH_RS_ARCHIVE_PAGE_SIZE",
    "IMMICH_RS_ARCHIVE_SELECTION",
    "IMMICH_RS_BUFFER_BYTES",
    "IMMICH_RS_CA_CERTIFICATE",
    "IMMICH_RS_CASE_SENSITIVE",
    "IMMICH_RS_COMPRESSION_RATIO_GRACE_BYTES",
    "IMMICH_RS_CONCURRENCY",
    "IMMICH_RS_CONFIG",
    "IMMICH_RS_INPUTS_JSON",
    "IMMICH_RS_LABEL",
    "IMMICH_RS_MAX_ARCHIVES",
    "IMMICH_RS_MAX_ARCHIVE_ENTRY_BYTES",
    "IMMICH_RS_MAX_ATTEMPTS_PER_OPERATION",
    "IMMICH_RS_MAX_COMPRESSION_RATIO",
    "IMMICH_RS_MAX_DIRECTORY_ENTRIES",
    "IMMICH_RS_MAX_ENTRIES",
    "IMMICH_RS_MAX_PATH_BYTES",
    "IMMICH_RS_MAX_RETRIES_PER_RUN",
    "IMMICH_RS_PICASA_ALBUMS",
    "IMMICH_RS_PICASA_FILENAME_DATE",
    "IMMICH_RS_RETRY_BASE_DELAY_MS",
    "IMMICH_RS_RETRY_DELAY_CAP_MS",
    "IMMICH_RS_SERVER",
    "IMMICH_RS_SOURCE",
    "IMMICH_RS_UPLOAD_CHECKPOINT",
    "IMMICH_RS_UPLOAD_DRY_RUN",
    "IMMICH_RS_UPLOAD_PLAN",
    "IMMICH_RS_UPLOAD_SOURCE",
    "IMMICH_RS_VERIFICATION_BUFFER_BYTES",
];

pub fn validate_names() -> Result<(), CliFailure> {
    for (name, _) in std::env::vars_os() {
        if let Some(name) = name.to_str()
            && name.starts_with(PREFIX)
            && !KNOWN_NAMES.contains(&name)
        {
            return Err(CliFailure::usage_owned(format!(
                "unknown immich-rs environment variable: {name}"
            )));
        }
    }
    Ok(())
}

pub fn config_path() -> Result<Option<PathBuf>, CliFailure> {
    value("IMMICH_RS_CONFIG").map(|value| value.map(PathBuf::from))
}

pub fn overlay(config: &mut EffectiveConfig) -> Result<(), CliFailure> {
    set_string(&mut config.server, "IMMICH_RS_SERVER")?;
    set_path(&mut config.ca_certificate, "IMMICH_RS_CA_CERTIFICATE")?;
    set_string(&mut config.label, "IMMICH_RS_LABEL")?;
    set_path(&mut config.source, "IMMICH_RS_SOURCE")?;
    if let Some(value) = value("IMMICH_RS_INPUTS_JSON")? {
        config.inputs = Some(
            serde_json::from_str(&value)
                .map_err(|_| CliFailure::usage("IMMICH_RS_INPUTS_JSON must be a JSON array"))?,
        );
    }
    set_parsed(&mut config.buffer_bytes, "IMMICH_RS_BUFFER_BYTES")?;
    set_parsed(&mut config.max_entries, "IMMICH_RS_MAX_ENTRIES")?;
    set_parsed(
        &mut config.max_directory_entries,
        "IMMICH_RS_MAX_DIRECTORY_ENTRIES",
    )?;
    set_parsed(&mut config.max_path_bytes, "IMMICH_RS_MAX_PATH_BYTES")?;
    set_bool(&mut config.case_sensitive, "IMMICH_RS_CASE_SENSITIVE")?;
    set_parsed(&mut config.max_archives, "IMMICH_RS_MAX_ARCHIVES")?;
    set_parsed(
        &mut config.max_archive_entry_bytes,
        "IMMICH_RS_MAX_ARCHIVE_ENTRY_BYTES",
    )?;
    set_parsed(
        &mut config.max_compression_ratio,
        "IMMICH_RS_MAX_COMPRESSION_RATIO",
    )?;
    set_parsed(
        &mut config.compression_ratio_grace_bytes,
        "IMMICH_RS_COMPRESSION_RATIO_GRACE_BYTES",
    )?;
    set_string(&mut config.album_mode, "IMMICH_RS_ALBUM_MODE")?;
    set_string(&mut config.album_path_joiner, "IMMICH_RS_ALBUM_PATH_JOINER")?;
    set_bool(&mut config.picasa_albums, "IMMICH_RS_PICASA_ALBUMS")?;
    set_bool(
        &mut config.picasa_filename_date,
        "IMMICH_RS_PICASA_FILENAME_DATE",
    )?;
    set_path(&mut config.upload_plan, "IMMICH_RS_UPLOAD_PLAN")?;
    set_path(&mut config.upload_source, "IMMICH_RS_UPLOAD_SOURCE")?;
    set_path(&mut config.upload_checkpoint, "IMMICH_RS_UPLOAD_CHECKPOINT")?;
    set_bool(&mut config.upload_dry_run, "IMMICH_RS_UPLOAD_DRY_RUN")?;
    set_parsed(
        &mut config.verification_buffer_bytes,
        "IMMICH_RS_VERIFICATION_BUFFER_BYTES",
    )?;
    set_parsed(&mut config.concurrency, "IMMICH_RS_CONCURRENCY")?;
    set_parsed(
        &mut config.max_attempts_per_operation,
        "IMMICH_RS_MAX_ATTEMPTS_PER_OPERATION",
    )?;
    set_parsed(
        &mut config.max_retries_per_run,
        "IMMICH_RS_MAX_RETRIES_PER_RUN",
    )?;
    set_parsed(
        &mut config.retry_base_delay_ms,
        "IMMICH_RS_RETRY_BASE_DELAY_MS",
    )?;
    set_parsed(
        &mut config.retry_delay_cap_ms,
        "IMMICH_RS_RETRY_DELAY_CAP_MS",
    )?;
    set_string(&mut config.archive_selection, "IMMICH_RS_ARCHIVE_SELECTION")?;
    set_bool(
        &mut config.archive_include_trashed,
        "IMMICH_RS_ARCHIVE_INCLUDE_TRASHED",
    )?;
    set_parsed(&mut config.archive_page_size, "IMMICH_RS_ARCHIVE_PAGE_SIZE")?;
    set_parsed(
        &mut config.archive_max_assets,
        "IMMICH_RS_ARCHIVE_MAX_ASSETS",
    )?;
    set_path(&mut config.archive_manifest, "IMMICH_RS_ARCHIVE_MANIFEST")?;
    set_path(
        &mut config.archive_destination,
        "IMMICH_RS_ARCHIVE_DESTINATION",
    )?;
    Ok(())
}

fn value(name: &str) -> Result<Option<String>, CliFailure> {
    std::env::var_os(name)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| CliFailure::usage_owned(format!("{name} must contain valid Unicode")))
        })
        .transpose()
}

fn set_string(target: &mut Option<String>, name: &str) -> Result<(), CliFailure> {
    if let Some(value) = value(name)? {
        *target = Some(value);
    }
    Ok(())
}

fn set_path(target: &mut Option<PathBuf>, name: &str) -> Result<(), CliFailure> {
    if let Some(value) = value(name)? {
        *target = Some(PathBuf::from(value));
    }
    Ok(())
}

fn set_parsed<T>(target: &mut Option<T>, name: &str) -> Result<(), CliFailure>
where
    T: FromStr,
{
    if let Some(value) = value(name)? {
        *target = Some(value.parse().map_err(|_| {
            CliFailure::usage_owned(format!("{name} has an invalid numeric value"))
        })?);
    }
    Ok(())
}

fn set_bool(target: &mut Option<bool>, name: &str) -> Result<(), CliFailure> {
    if let Some(value) = value(name)? {
        *target = Some(match value.as_str() {
            "true" => true,
            "false" => false,
            _ => {
                return Err(CliFailure::usage_owned(format!(
                    "{name} must be true or false"
                )));
            }
        });
    }
    Ok(())
}
