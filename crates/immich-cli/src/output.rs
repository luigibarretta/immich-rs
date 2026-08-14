use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::Path;

use immich_rs_core::UploadPlan;

use crate::failure::CliFailure;

const MAX_PLAN_BYTES: u64 = 128 * 1_024 * 1_024;

pub fn write_json(value: &impl serde::Serialize, label: &str) -> Result<(), CliFailure> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value)
        .map_err(|_| CliFailure::invariant(&format!("cannot serialize {label}")))?;
    writeln!(output).map_err(|_| CliFailure::invariant(&format!("cannot write {label}")))
}

pub fn load_upload_plan(path: &Path) -> Result<UploadPlan, CliFailure> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| CliFailure::usage("cannot read upload plan"))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_PLAN_BYTES
    {
        return Err(CliFailure::usage(
            "upload plan must be a bounded regular file",
        ));
    }
    let file = File::open(path).map_err(|_| CliFailure::usage("cannot read upload plan"))?;
    let plan: UploadPlan = serde_json::from_reader(BufReader::new(file))
        .map_err(|_| CliFailure::usage("invalid upload plan JSON"))?;
    plan.validate()
        .map_err(|_| CliFailure::usage("invalid upload plan"))?;
    Ok(plan)
}
