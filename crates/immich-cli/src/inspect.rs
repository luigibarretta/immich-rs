use std::ffi::OsString;
use std::path::PathBuf;

use immich_rs_client::upload_plan_sha256;
use serde::Serialize;

use crate::config::EffectiveConfig;
use crate::failure::CliFailure;
use crate::{args, output};

const INSPECTION_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct UploadPlanInspection {
    schema_version: u32,
    plan_sha256: String,
    operations: u64,
    media_bytes: u64,
}

pub fn run_upload_plan(
    arguments: &[OsString],
    effective: &EffectiveConfig,
) -> Result<(), CliFailure> {
    let path = parse_path(arguments, effective.upload_plan.clone())?;
    let plan = output::load_upload_plan(&path)?;
    let plan_sha256 = upload_plan_sha256(&plan)
        .map_err(|_| CliFailure::invariant("cannot digest upload plan"))?;
    output::write_json(
        &UploadPlanInspection {
            schema_version: INSPECTION_SCHEMA_VERSION,
            plan_sha256,
            operations: plan.summary.operations,
            media_bytes: plan.summary.media_bytes,
        },
        "upload plan inspection",
    )
}

fn parse_path(arguments: &[OsString], configured: Option<PathBuf>) -> Result<PathBuf, CliFailure> {
    let mut path = configured;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--plan") => path = Some(args::path_value(arguments, index, "--plan")?),
            _ => return Err(CliFailure::usage("unsupported inspect option")),
        }
        index += 2;
    }
    path.ok_or_else(|| CliFailure::usage("--plan is required"))
}
