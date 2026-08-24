#![forbid(unsafe_code)]
//! Version-aware, bounded Immich HTTP capabilities.

mod archive;
mod auth;
mod endpoint;
mod error;
mod import;
mod import_models;
mod migration;
mod models;
mod production;
mod read;
mod response;
mod tls;
mod upload;

pub use archive::{ArchiveDownload, ArchiveListConfig, ArchiveVisibility, RemoteArchiveAsset};
pub use auth::{ApiKey, ApiKeyError};
pub use endpoint::{EndpointAccess, EndpointError, ImmichEndpoint};
pub use error::{ClientError, ClientErrorClass};
pub use import::{ImmichImportClient, RemoteAlbum};
pub use migration::{
    MigrationListConfig, RemoteMigrationAsset, RemoteMigrationInventory, RemoteOwnedAlbum,
};
pub use production::{
    ProductionImmichImportClient, ProductionImmichUploadClient, ProductionImportAuthorization,
    ProductionUploadAuthorization, upload_plan_sha256,
};
pub use read::{ClientConfig, ImmichReadClient, NegotiatedServer};
pub use tls::TlsRootCertificates;
pub use upload::{DuplicateCheck, ImmichUploadClient, UploadRequest, UploadResult};

/// Human-readable component identity.
pub const COMPONENT: &str = "immich-rs-client";

#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod tests;
