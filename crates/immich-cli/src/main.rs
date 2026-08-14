#![forbid(unsafe_code)]

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("immich-rs {VERSION}");
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            println!(
                "immich-rs {VERSION}\n\nArchitecture scaffold only; no media command is implemented yet."
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "immich-rs is an architecture scaffold and cannot process media yet; see ROADMAP.md"
            );
            ExitCode::from(2)
        }
    }
}
