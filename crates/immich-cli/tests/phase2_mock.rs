#![forbid(unsafe_code)]

use std::path::Path;
use std::process::Command;

#[test]
fn phase_two_converges_against_only_the_synthetic_mock() -> Result<(), Box<dyn std::error::Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot resolve repository root")?;
    let output = Command::new("python3")
        .arg(repository.join("tests/integration/run_phase2_mock.py"))
        .arg(env!("CARGO_BIN_EXE_immich-rs"))
        .output()?;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}
