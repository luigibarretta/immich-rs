#![forbid(unsafe_code)]

mod apple_photos;
mod archive_apply;
mod archive_plan;
mod args;
mod failure;
mod folder;
mod google_takeout;
mod network;
mod output;
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
    match run(&arguments).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("{}", failure.message());
            ExitCode::from(failure.exit_code())
        }
    }
}

async fn run(arguments: &[OsString]) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("--version" | "-V") if arguments.len() == 1 => {
            println!("immich-rs {VERSION}");
            Ok(())
        }
        Some("--help" | "-h" | "help") if arguments.len() == 1 => {
            print_help();
            Ok(())
        }
        Some("plan") => run_plan(&arguments[1..]).await,
        Some("apply") => run_apply(&arguments[1..]).await,
        _ => Err(CliFailure::usage(
            "unsupported or missing command; run --help",
        )),
    }
}

async fn run_plan(arguments: &[OsString]) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("folder") => folder::run(&args::parse_folder(&arguments[1..])?),
        Some("google-takeout") => {
            google_takeout::run(&args::parse_google_takeout(&arguments[1..])?)
        }
        Some("apple-photos") => apple_photos::run(&args::parse_apple_photos(&arguments[1..])?),
        Some("upload")
            if arguments.get(1).and_then(|argument| argument.to_str()) == Some("folder") =>
        {
            upload_plan::run(args::parse_upload_folder(&arguments[2..])?).await
        }
        Some("archive")
            if arguments.get(1).and_then(|argument| argument.to_str()) == Some("immich") =>
        {
            archive_plan::run(args::parse_archive_plan(&arguments[2..])?).await
        }
        _ => Err(CliFailure::usage(
            "plan supports folder, google-takeout, apple-photos, upload folder and archive immich",
        )),
    }
}

async fn run_apply(arguments: &[OsString]) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("upload") => {
            let request = args::parse_apply(&arguments[1..])?;
            if request.dry_run {
                upload_dry_run::run(&request)
            } else {
                upload_apply::run(request).await
            }
        }
        Some("archive") => archive_apply::run(args::parse_archive_apply(&arguments[1..])?).await,
        _ => Err(CliFailure::usage("apply supports upload and archive")),
    }
}

fn print_help() {
    println!(
        "immich-rs {VERSION}\n\nBounded planning, disposable folder upload and verified local archive\n\nUsage:\n  immich-rs plan folder [OPTIONS] <PATH>\n  immich-rs plan google-takeout [OPTIONS] <DIRECTORY|ZIP...>\n  immich-rs plan apple-photos [APPLE_OPTIONS] <DIRECTORY|ZIP...>\n  immich-rs plan upload folder --server <LOOPBACK_URL> [OPTIONS] <PATH>\n  immich-rs plan archive immich --server <LOOPBACK_URL> [ARCHIVE_OPTIONS]\n  immich-rs apply upload --dry-run --plan <FILE> --source <PATH> --checkpoint <FILE> [SCAN_OPTIONS]\n  immich-rs apply upload --server <LOOPBACK_URL> --plan <FILE> --source <PATH> --checkpoint <FILE> [SCAN_OPTIONS]\n  immich-rs apply archive --server <LOOPBACK_URL> --manifest <FILE> --destination <PATH>\n\nScan options:\n  --label <LABEL>\n  --buffer-bytes <BYTES>\n  --max-entries <COUNT>\n  --max-directory-entries <COUNT>\n\nApple options:\n  --album-mode <none|folder|path>\n  --album-path-joiner <TEXT>\n\nArchive options:\n  --selection <timeline|archive|hidden|all>\n  --include-trashed\n  --page-size <1..1000>\n  --max-assets <COUNT>\n\nGoogle Takeout and Apple planning accept one directory or up to 64 independent ZIP parts and never apply them.\nThe API key is read only from IMMICH_RS_API_KEY. Server commands reject non-loopback origins.\nNo delete, replace or metadata mutation command exists."
    );
}
