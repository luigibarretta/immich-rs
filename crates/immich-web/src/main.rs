#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::ExitCode;

use immich_rs_web::{WebConfig, WebConsole};

const CONFIG_ENV: &str = "IMMICH_RS_WEB_CONFIG";

enum Command {
    Help,
    Run(PathBuf),
    Version,
}

#[tokio::main]
async fn main() -> ExitCode {
    match command().map(run_command) {
        Ok(Some(path)) => match run(path).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("immich-rs-web failed: {message}");
                ExitCode::FAILURE
            }
        },
        Ok(None) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("immich-rs-web failed: {message}");
            ExitCode::FAILURE
        }
    }
}

fn command() -> Result<Command, &'static str> {
    validate_environment()?;
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [] => env::var_os(CONFIG_ENV)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .map(Command::Run)
            .ok_or("configuration selector is required"),
        [flag] if flag == "--help" || flag == "-h" => Ok(Command::Help),
        [flag] if flag == "--version" || flag == "-V" => Ok(Command::Version),
        [flag, path] if flag == "--config" && !path.is_empty() => {
            Ok(Command::Run(PathBuf::from(path)))
        }
        _ => Err("arguments must be --config PATH, --help or --version"),
    }
}

fn run_command(command: Command) -> Option<PathBuf> {
    match command {
        Command::Help => {
            println!(
                "immich-rs-web {}\n\nUsage: immich-rs-web [--config PATH]\n\nEnvironment:\n  {CONFIG_ENV}  Strict operator configuration file",
                env!("CARGO_PKG_VERSION")
            );
            None
        }
        Command::Version => {
            println!("immich-rs-web {}", env!("CARGO_PKG_VERSION"));
            None
        }
        Command::Run(path) => Some(path),
    }
}

fn validate_environment() -> Result<(), &'static str> {
    for (key, _) in env::vars_os() {
        if key != OsStr::new(CONFIG_ENV)
            && key
                .to_str()
                .is_some_and(|value| value.starts_with("IMMICH_RS_WEB_"))
        {
            return Err("unknown IMMICH_RS_WEB_* environment variable");
        }
    }
    Ok(())
}

async fn run(path: PathBuf) -> Result<(), String> {
    let config = WebConfig::load(&path).map_err(|error| error.to_string())?;
    let console = WebConsole::from_config(config).map_err(|error| error.to_string())?;
    console
        .serve(async {
            let _signal_result = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| "web listener stopped unexpectedly".to_owned())
}
