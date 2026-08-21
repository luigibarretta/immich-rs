#![forbid(unsafe_code)]
//! Bounded, source-verified and crash-recoverable upload execution.

mod apply;
mod archive_apply;
mod archive_plan;
mod config;
mod error;
mod journal;
mod operation;
mod planner;
mod retry;
mod verify;

pub use apply::{apply_upload, dry_run_upload};
pub use archive_apply::apply_archive;
pub use archive_plan::{ArchivePlanningConfig, ArchiveSelection, create_archive_manifest};
pub use config::UploadExecutionConfig;
pub use error::{ExecutorError, ExecutorErrorClass};
pub use planner::create_upload_plan;

/// Human-readable component identity.
pub const COMPONENT: &str = "immich-rs-executor";

#[cfg(test)]
mod tests;
