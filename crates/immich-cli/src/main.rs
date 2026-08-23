#![forbid(unsafe_code)]

mod apple_photos;
mod apply_args;
mod archive_apply;
mod archive_plan;
mod args;
mod config;
mod environment;
mod failure;
mod folder;
mod google_takeout;
mod network;
mod output;
mod plan_args;
mod signal;
mod upload_apply;
mod upload_dry_run;
mod upload_plan;

use std::ffi::OsString;
use std::process::ExitCode;

use failure::CliFailure;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let result = match config::load(&arguments) {
        Ok((configuration, filtered)) => run(&filtered, &configuration).await,
        Err(failure) => Err(failure),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("{}", failure.message());
            ExitCode::from(failure.exit_code())
        }
    }
}

async fn run(
    arguments: &[OsString],
    configuration: &config::EffectiveConfig,
) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("--version" | "-V") if arguments.len() == 1 => {
            println!("immich-rs {VERSION}");
            Ok(())
        }
        Some("--help" | "-h" | "help") if arguments.len() == 1 => {
            print_help();
            Ok(())
        }
        Some("config")
            if arguments.get(1).and_then(|argument| argument.to_str()) == Some("show")
                && arguments.len() == 2 =>
        {
            output::write_json(configuration, "effective configuration")
        }
        Some("plan") => run_plan(&arguments[1..], configuration).await,
        Some("apply") => run_apply(&arguments[1..], configuration).await,
        _ => Err(CliFailure::usage(
            "unsupported or missing command; run --help",
        )),
    }
}

async fn run_plan(
    arguments: &[OsString],
    configuration: &config::EffectiveConfig,
) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("folder") => folder::run(&plan_args::parse_folder(&arguments[1..], configuration)?),
        Some("google-takeout") => google_takeout::run(&plan_args::parse_google_takeout(
            &arguments[1..],
            configuration,
        )?),
        Some("apple-photos") => apple_photos::run(&plan_args::parse_apple_photos(
            &arguments[1..],
            configuration,
        )?),
        Some("upload")
            if arguments.get(1).and_then(|argument| argument.to_str()) == Some("folder") =>
        {
            upload_plan::run(plan_args::parse_upload_folder(
                &arguments[2..],
                configuration,
            )?)
            .await
        }
        Some("archive")
            if arguments.get(1).and_then(|argument| argument.to_str()) == Some("immich") =>
        {
            archive_plan::run(plan_args::parse_archive_plan(
                &arguments[2..],
                configuration,
            )?)
            .await
        }
        _ => Err(CliFailure::usage(
            "plan supports folder, google-takeout, apple-photos, upload folder and archive immich",
        )),
    }
}

async fn run_apply(
    arguments: &[OsString],
    configuration: &config::EffectiveConfig,
) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("upload") => {
            let request = apply_args::parse_upload(&arguments[1..], configuration)?;
            if request.dry_run {
                upload_dry_run::run(&request)
            } else {
                upload_apply::run(request).await
            }
        }
        Some("archive") => {
            archive_apply::run(apply_args::parse_archive(&arguments[1..], configuration)?).await
        }
        _ => Err(CliFailure::usage("apply supports upload and archive")),
    }
}

fn print_help() {
    println!(
        "immich-rs {VERSION}\n\nBounded planning, disposable folder upload and verified local archive\n\nUsage:\n  immich-rs [--config <FILE>] config show\n  immich-rs [--config <FILE>] plan folder [OPTIONS] [PATH]\n  immich-rs [--config <FILE>] plan google-takeout [OPTIONS] [DIRECTORY|ZIP...]\n  immich-rs [--config <FILE>] plan apple-photos [APPLE_OPTIONS] [DIRECTORY|ZIP...]\n  immich-rs [--config <FILE>] plan upload folder [OPTIONS] [PATH]\n  immich-rs [--config <FILE>] plan archive immich [ARCHIVE_OPTIONS]\n  immich-rs [--config <FILE>] apply upload [UPLOAD_OPTIONS]\n  immich-rs [--config <FILE>] apply archive [ARCHIVE_OPTIONS]\n\nScan options:\n  --label <LABEL>\n  --buffer-bytes <BYTES>\n  --max-entries <COUNT>\n  --max-directory-entries <COUNT>\n  --max-path-bytes <BYTES>\n  --case-sensitive | --case-insensitive\n\nRemote read-only server commands require HTTPS and --authorize-production-read.\nProduction authorization flags are CLI-only and are never read from configuration.\nFormat and executor resource limits are documented in docs/configuration.md.\nBoolean options have explicit positive and negative CLI forms where inheritance matters.\nOther options can use CLI, IMMICH_RS_* environment or strict schema-v1 TOML configuration.\nAPI keys use IMMICH_RS_API_KEY or IMMICH_RS_API_KEY_FILE and are never accepted in TOML.\nNo delete, replace or metadata mutation command exists."
    );
}
