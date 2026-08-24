use std::ffi::OsString;

use crate::args::{self, MigrationApplyRequest};
use crate::config::EffectiveConfig;
use crate::failure::CliFailure;
use crate::migration_plan_args;

pub fn parse(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<MigrationApplyRequest, CliFailure> {
    let mut plan = effective.migration_plan.clone();
    let mut checkpoint = effective.migration_checkpoint.clone();
    let mut source_server = effective.migration_source_server.clone();
    let mut destination_server = effective.migration_destination_server.clone();
    let mut dry_run = effective.migration_dry_run.unwrap_or(false);
    let mut server_from_cli = false;
    let mut config = migration_plan_args::configured(effective);
    let mut index = 0;
    while index < arguments.len() {
        if let Some(consumed) = migration_plan_args::config_option(arguments, index, &mut config)? {
            index += consumed;
            continue;
        }
        match arguments[index].to_str() {
            Some("--plan") => plan = Some(args::path_value(arguments, index, "--plan")?),
            Some("--checkpoint") => {
                checkpoint = Some(args::path_value(arguments, index, "--checkpoint")?);
            }
            Some("--source-server") => {
                source_server = Some(args::string_value(arguments, index, "--source-server")?);
                server_from_cli = true;
            }
            Some("--destination-server") => {
                destination_server = Some(args::string_value(
                    arguments,
                    index,
                    "--destination-server",
                )?);
                server_from_cli = true;
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
            _ => return Err(CliFailure::usage("unsupported migration apply option")),
        }
        index += 2;
    }
    config = config
        .validate()
        .map_err(|_| CliFailure::usage("invalid migration resource limits"))?;
    if dry_run && server_from_cli {
        return Err(CliFailure::usage(
            "migration dry-run does not accept server transport options",
        ));
    }
    if !dry_run && (source_server.is_none() || destination_server.is_none()) {
        return Err(CliFailure::usage(
            "--source-server and --destination-server are required for apply",
        ));
    }
    if !dry_run && checkpoint.is_none() {
        return Err(CliFailure::usage("--checkpoint is required for apply"));
    }
    Ok(MigrationApplyRequest {
        plan: plan.ok_or_else(|| CliFailure::usage("--plan is required"))?,
        checkpoint: if dry_run { None } else { checkpoint },
        source_server: if dry_run { None } else { source_server },
        destination_server: if dry_run { None } else { destination_server },
        dry_run,
        config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn dry_run_rejects_cli_transport() {
        let result = parse(
            &values(&[
                "--plan",
                "plan.json",
                "--dry-run",
                "--source-server",
                "http://127.0.0.1:1",
            ]),
            &EffectiveConfig::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn live_apply_requires_both_servers_and_checkpoint() {
        let result = parse(
            &values(&["--plan", "plan.json", "--no-dry-run"]),
            &EffectiveConfig::default(),
        );
        assert!(result.is_err());
    }
}
