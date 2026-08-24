#![forbid(unsafe_code)]
//! Bounded, source-verified and crash-recoverable upload execution.

mod apply;
mod archive_apply;
mod archive_plan;
mod config;
mod error;
mod import_apply;
mod import_config;
mod import_dry_run;
mod import_effect_support;
mod import_effects;
mod import_journal;
mod import_journal_db;
mod import_planner;
mod import_staging;
mod import_staging_fs;
mod journal;
mod operation;
mod planner;
mod retry;
mod verify;

pub use apply::{apply_production_upload, apply_upload, dry_run_upload};
pub use archive_apply::apply_archive;
pub use archive_plan::{ArchivePlanningConfig, ArchiveSelection, create_archive_manifest};
pub use config::UploadExecutionConfig;
pub use error::{ExecutorError, ExecutorErrorClass};
pub use import_apply::{
    apply_apple_photos_import, apply_production_apple_photos_import,
    apply_production_takeout_import, apply_takeout_import,
};
pub use import_config::{ApplePhotosImportConfig, TakeoutImportConfig};
pub use import_dry_run::{dry_run_apple_photos_import, dry_run_takeout_import};
pub use import_planner::{create_apple_photos_upload_plan, create_takeout_upload_plan};
pub use planner::create_upload_plan;

/// Human-readable component identity.
pub const COMPONENT: &str = "immich-rs-executor";

#[cfg(test)]
mod apple_import_tests;
#[cfg(test)]
mod import_staging_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod tests;
