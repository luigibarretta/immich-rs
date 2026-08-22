#![forbid(unsafe_code)]

mod support;

use std::path::Path;

#[test]
fn phase_five_archives_only_synthetic_originals() -> Result<(), Box<dyn std::error::Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot resolve repository root")?;
    let output = support::python()
        .arg(repository.join("tests/integration/run_phase5_mock.py"))
        .arg(env!("CARGO_BIN_EXE_immich-rs"))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "Phase-5 mock subprocess failed with {}: stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
        .into());
    }
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}
