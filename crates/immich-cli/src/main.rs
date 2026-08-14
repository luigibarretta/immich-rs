#![forbid(unsafe_code)]

mod args;
mod failure;
mod folder;
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
        Some("upload")
            if arguments.get(1).and_then(|argument| argument.to_str()) == Some("folder") =>
        {
            upload_plan::run(args::parse_upload_folder(&arguments[2..])?).await
        }
        _ => Err(CliFailure::usage(
            "plan supports 'folder' and 'upload folder'",
        )),
    }
}

async fn run_apply(arguments: &[OsString]) -> Result<(), CliFailure> {
    if arguments.first().and_then(|argument| argument.to_str()) != Some("upload") {
        return Err(CliFailure::usage("apply supports only 'upload'"));
    }
    let request = args::parse_apply(&arguments[1..])?;
    if request.dry_run {
        upload_dry_run::run(&request)
    } else {
        upload_apply::run(request).await
    }
}

fn print_help() {
    println!(
        "immich-rs {VERSION}\n\nBounded folder planning and disposable Phase-2 upload\n\nUsage:\n  immich-rs plan folder [OPTIONS] <PATH>\n  immich-rs plan upload folder --server <LOOPBACK_URL> [OPTIONS] <PATH>\n  immich-rs apply upload --dry-run --plan <FILE> --source <PATH> --checkpoint <FILE> [SCAN_OPTIONS]\n  immich-rs apply upload --server <LOOPBACK_URL> --plan <FILE> --source <PATH> --checkpoint <FILE> [SCAN_OPTIONS]\n\nScan options:\n  --label <LABEL>\n  --buffer-bytes <BYTES>\n  --max-entries <COUNT>\n  --max-directory-entries <COUNT>\n\nRepeat non-default scan limits when applying a plan. The API key is read only from IMMICH_RS_API_KEY. Phase 2 rejects non-loopback servers.\nNo delete, replace or metadata mutation command exists."
    );
}
