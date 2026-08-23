#![forbid(unsafe_code)]
//! Version-aware, bounded Immich HTTP capabilities.

mod archive;
mod auth;
mod endpoint;
mod error;
mod models;
mod read;
mod response;
mod upload;

pub use archive::{ArchiveDownload, ArchiveListConfig, ArchiveVisibility, RemoteArchiveAsset};
pub use auth::{ApiKey, ApiKeyError};
pub use endpoint::{EndpointAccess, EndpointError, ImmichEndpoint};
pub use error::{ClientError, ClientErrorClass};
pub use read::{ClientConfig, ImmichReadClient, NegotiatedServer};
pub use upload::{DuplicateCheck, ImmichUploadClient, UploadRequest, UploadResult};

/// Human-readable component identity.
pub const COMPONENT: &str = "immich-rs-client";

#[cfg(test)]
mod tests;
