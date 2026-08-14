#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use immich_rs_core::CancellationToken;
use immich_rs_sources::{FolderScanConfig, NoProgress, ScanError, scan_folder};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USAGE_EXIT: u8 = 2;
const SOURCE_EXIT: u8 = 4;
const INVARIANT_EXIT: u8 = 70;
const CANCELLED_EXIT: u8 = 130;

fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("{}", failure.message);
            ExitCode::from(failure.exit_code)
        }
    }
}

fn run(arguments: &[OsString]) -> Result<(), CliFailure> {
    match arguments.first().and_then(|argument| argument.to_str()) {
        Some("--version" | "-V") if arguments.len() == 1 => {
            println!("immich-rs {VERSION}");
            Ok(())
        }
        Some("--help" | "-h" | "help") if arguments.len() == 1 => {
            print_help();
            Ok(())
        }
        Some("plan") => run_plan(&arguments[1..]),
        _ => Err(CliFailure::usage(
            "unsupported or missing command; run --help",
        )),
    }
}

fn run_plan(arguments: &[OsString]) -> Result<(), CliFailure> {
    if arguments.first().and_then(|argument| argument.to_str()) != Some("folder") {
        return Err(CliFailure::usage(
            "only the read-only 'plan folder' command is supported",
        ));
    }
    let request = parse_folder_request(&arguments[1..])?;
    let cancellation = CancellationToken::default();
    install_interrupt_handler(cancellation.clone())?;
    let plan = scan_folder(
        &request.root,
        &request.label,
        &request.config,
        &cancellation,
        &mut NoProgress,
    )
    .map_err(|error| CliFailure::from_scan(&error))?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, &plan)
        .map_err(|_| CliFailure::invariant("cannot serialize normalized plan"))?;
    writeln!(output).map_err(|_| CliFailure::invariant("cannot write normalized plan"))?;
    Ok(())
}

struct FolderRequest {
    root: PathBuf,
    label: String,
    config: FolderScanConfig,
}

fn parse_folder_request(arguments: &[OsString]) -> Result<FolderRequest, CliFailure> {
    let mut label = "folder".to_owned();
    let mut config = FolderScanConfig::default();
    let mut root = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        match argument.to_str() {
            Some("--label") => {
                label = string_value(arguments, index, "--label")?;
                index += 2;
            }
            Some("--buffer-bytes") => {
                config.buffer_bytes = usize_value(arguments, index, "--buffer-bytes")?;
                index += 2;
            }
            Some("--max-entries") => {
                config.max_entries = usize_value(arguments, index, "--max-entries")?;
                index += 2;
            }
            Some("--max-directory-entries") => {
                config.max_directory_entries =
                    usize_value(arguments, index, "--max-directory-entries")?;
                index += 2;
            }
            Some(value) if value.starts_with('-') => {
                return Err(CliFailure::usage("unsupported plan option"));
            }
            _ if root.is_none() => {
                root = Some(PathBuf::from(argument));
                index += 1;
            }
            _ => {
                return Err(CliFailure::usage(
                    "plan folder accepts exactly one source path",
                ));
            }
        }
    }
    let Some(root) = root else {
        return Err(CliFailure::usage("plan folder requires a source path"));
    };
    Ok(FolderRequest {
        root,
        label,
        config,
    })
}

fn string_value(arguments: &[OsString], index: usize, option: &str) -> Result<String, CliFailure> {
    let Some(value) = arguments.get(index + 1).and_then(|value| value.to_str()) else {
        return Err(CliFailure::usage_owned(format!(
            "{option} requires a Unicode value"
        )));
    };
    Ok(value.to_owned())
}

fn usize_value(arguments: &[OsString], index: usize, option: &str) -> Result<usize, CliFailure> {
    let value = string_value(arguments, index, option)?;
    value
        .parse::<usize>()
        .map_err(|_| CliFailure::usage_owned(format!("{option} requires a positive integer")))
}

fn install_interrupt_handler(cancellation: CancellationToken) -> Result<(), CliFailure> {
    let interrupts = Arc::new(AtomicU8::new(0));
    let handler_interrupts = Arc::clone(&interrupts);
    ctrlc::set_handler(move || {
        let previous = handler_interrupts.fetch_add(1, Ordering::AcqRel);
        if previous == 0 {
            cancellation.cancel();
        } else {
            std::process::exit(i32::from(CANCELLED_EXIT));
        }
    })
    .map_err(|_| CliFailure::invariant("cannot install cancellation handler"))
}

fn print_help() {
    println!(
        "immich-rs {VERSION}\n\nRead-only normalized planning\n\nUsage:\n  immich-rs plan folder [OPTIONS] <PATH>\n\nOptions:\n  --label <LABEL>                    Non-secret source label (default: folder)\n  --buffer-bytes <BYTES>             Streaming buffer, 4096..=4194304\n  --max-entries <COUNT>              Maximum retained source entries\n  --max-directory-entries <COUNT>    Maximum entries in one directory\n  -h, --help                         Print help\n  -V, --version                      Print version\n\nNo upload, delete, replace or metadata mutation command exists."
    );
}

struct CliFailure {
    exit_code: u8,
    message: String,
}

impl CliFailure {
    fn usage(message: &str) -> Self {
        Self::usage_owned(message.to_owned())
    }

    const fn usage_owned(message: String) -> Self {
        Self {
            exit_code: USAGE_EXIT,
            message,
        }
    }

    fn invariant(message: &str) -> Self {
        Self {
            exit_code: INVARIANT_EXIT,
            message: message.to_owned(),
        }
    }

    fn from_scan(error: &ScanError) -> Self {
        let exit_code = match error {
            ScanError::Cancelled => CANCELLED_EXIT,
            ScanError::InvalidPlan(_) => INVARIANT_EXIT,
            ScanError::InvalidConfiguration(_)
            | ScanError::InvalidRoot
            | ScanError::LimitExceeded(_) => SOURCE_EXIT,
        };
        Self {
            exit_code,
            message: error.to_string(),
        }
    }
}
