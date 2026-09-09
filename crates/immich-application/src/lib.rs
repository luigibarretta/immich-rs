#![forbid(unsafe_code)]
//! Typed workflow composition shared by the CLI and optional Web Console.
//!
//! The facade intentionally exposes no server mutation capability in its
//! read-only folder module:
//!
//! ```compile_fail
//! use immich_rs_application::ImmichUploadClient;
//! ```

mod folder;
mod progress;

pub use folder::{FolderPlanRequest, plan_folder};
pub use immich_rs_core::{Cancellation, CancellationToken, NormalizedPlan, ProgressStage};
pub use immich_rs_sources::{FolderScanConfig, ScanError};
pub use progress::{
    APPLICATION_PROGRESS_SCHEMA_VERSION, ApplicationNoProgress, ApplicationProgressEvent,
    ApplicationProgressObserver,
};
