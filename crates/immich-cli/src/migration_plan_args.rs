use std::ffi::OsString;

use immich_rs_application::MigrationPlanningConfig;

use crate::args::{self, MigrationPlanRequest};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;

pub fn parse(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<MigrationPlanRequest, CliFailure> {
    let mut source_server = effective.migration_source_server.clone();
    let mut destination_server = effective.migration_destination_server.clone();
    let mut config = configured(effective);
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = config_option(arguments, index, &mut config)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--source-server") => {
                source_server = Some(args::string_value(arguments, index, "--source-server")?);
            }
            Some("--destination-server") => {
                destination_server = Some(args::string_value(
                    arguments,
                    index,
                    "--destination-server",
                )?);
            }
            _ => return Err(CliFailure::usage("unsupported migration plan option")),
        }
        index += 2;
    }
    config = config
        .validate()
        .map_err(|_| CliFailure::usage("invalid migration resource limits"))?;
    Ok(MigrationPlanRequest {
        source_server: source_server
            .ok_or_else(|| CliFailure::usage("--source-server is required"))?,
        destination_server: destination_server
            .ok_or_else(|| CliFailure::usage("--destination-server is required"))?,
        config,
    })
}

pub fn configured(effective: &EffectiveConfig) -> MigrationPlanningConfig {
    let mut config = MigrationPlanningConfig {
        upload: args::upload_config(effective),
        ..MigrationPlanningConfig::default()
    };
    config.inventory.page_size = effective
        .migration_page_size
        .unwrap_or(config.inventory.page_size);
    config.inventory.max_assets = effective
        .migration_max_assets
        .unwrap_or(config.inventory.max_assets);
    config.inventory.max_albums = effective
        .migration_max_albums
        .unwrap_or(config.inventory.max_albums);
    config.inventory.max_album_memberships = effective
        .migration_max_album_memberships
        .unwrap_or(config.inventory.max_album_memberships);
    config.max_asset_bytes = effective
        .migration_max_asset_bytes
        .unwrap_or(config.max_asset_bytes);
    config.max_total_bytes = effective
        .migration_max_total_bytes
        .unwrap_or(config.max_total_bytes);
    config
}

pub fn config_option(
    arguments: &[OsString],
    index: usize,
    config: &mut MigrationPlanningConfig,
) -> Result<Option<usize>, CliFailure> {
    if let Some(consumed) =
        crate::apply_args::execution_option(arguments, index, &mut config.upload)?
    {
        return Ok(Some(consumed));
    }
    match arguments[index].to_str() {
        Some("--page-size") => {
            config.inventory.page_size = args::usize_value(arguments, index, "--page-size")?;
        }
        Some("--max-assets") => {
            config.inventory.max_assets = args::usize_value(arguments, index, "--max-assets")?;
        }
        Some("--max-albums") => {
            config.inventory.max_albums = args::usize_value(arguments, index, "--max-albums")?;
        }
        Some("--max-album-memberships") => {
            config.inventory.max_album_memberships =
                args::usize_value(arguments, index, "--max-album-memberships")?;
        }
        Some("--max-asset-bytes") => {
            config.max_asset_bytes = args::u64_value(arguments, index, "--max-asset-bytes")?;
        }
        Some("--max-total-bytes") => {
            config.max_total_bytes = args::u64_value(arguments, index, "--max-total-bytes")?;
        }
        _ => return Ok(None),
    }
    Ok(Some(2))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::parse;
    use crate::config::EffectiveConfig;

    #[test]
    fn both_servers_and_bounded_limits_are_required() {
        let args = [
            "--source-server",
            "http://127.0.0.1:2284",
            "--destination-server",
            "http://127.0.0.1:2285",
            "--max-assets",
            "2",
        ]
        .map(OsString::from);
        let request = parse(&args, &EffectiveConfig::default());
        assert!(matches!(request, Ok(value) if value.config.inventory.max_assets == 2));
        assert!(parse(&[], &EffectiveConfig::default()).is_err());
    }
}
