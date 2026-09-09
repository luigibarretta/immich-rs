#![forbid(unsafe_code)]
//! Typed workflow composition shared by the CLI and optional Web Console.
//!
//! The facade intentionally exposes no server mutation capability in its
//! read-only folder module:
//!
//! ```compile_fail
//! use immich_rs_application::ImmichUploadClient;
//! ```

mod error;
mod folder;
mod progress;
mod source_plans;
mod upload_apply;
mod upload_execution;
mod upload_plans;

pub use error::{ApplicationError, ApplicationErrorClass};
pub use folder::{FolderPlanRequest, plan_folder};
pub use immich_rs_core::{Cancellation, CancellationToken, NormalizedPlan, ProgressStage};
pub use immich_rs_sources::{ApplePhotosScanConfig, PicasaScanConfig, TakeoutScanConfig};
pub use immich_rs_sources::{FolderScanConfig, ScanError};
pub use progress::{
    APPLICATION_PROGRESS_SCHEMA_VERSION, ApplicationNoProgress, ApplicationProgressEvent,
    ApplicationProgressObserver,
};
pub use source_plans::{SourcePlanRequest, plan_apple_photos, plan_google_takeout, plan_picasa};
pub use upload_apply::{
    PreparedUploadApply, ProductionWriteRequest, UploadApplyReport, UploadApplyRequest,
    apply_prepared_upload, prepare_upload_apply,
};
pub use upload_execution::{UploadDryRunReport, UploadDryRunRequest, dry_run_upload_plan};
pub use upload_plans::{
    FolderUploadPlanRequest, SourceUploadPlanRequest, plan_apple_photos_upload, plan_folder_upload,
    plan_google_takeout_upload, plan_picasa_upload,
};
