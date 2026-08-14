#![forbid(unsafe_code)]
//! Bounded, source-verified and crash-recoverable upload execution.

mod apply;
mod config;
mod error;
mod journal;
mod operation;
mod planner;
mod retry;
mod verify;

pub use apply::{apply_upload, dry_run_upload};
pub use config::UploadExecutionConfig;
pub use error::{ExecutorError, ExecutorErrorClass};
pub use planner::create_upload_plan;

/// Human-readable component identity.
pub const COMPONENT: &str = "immich-rs-executor";

#[cfg(test)]
mod tests;
