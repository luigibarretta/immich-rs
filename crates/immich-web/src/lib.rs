#![forbid(unsafe_code)]
//! Authenticated, bounded operator Web Console policy and HTTP surface.

mod auth;
#[cfg(test)]
mod auth_tests;
mod config;
mod cookies;
mod error;
mod events_http;
mod grants;
mod http;
mod job_http;
mod jobs;
#[cfg(test)]
mod jobs_tests;
mod limits;
mod oidc;
mod oidc_http;
mod policy;
mod profiles;
mod server;
mod state_store;
mod tls;
mod views;

pub use config::WebConfig;
pub use error::WebConfigError;
pub use http::WebConsole;
pub use limits::WebLimits;
pub use profiles::{
    ResolvedSourceProfile, ResolvedStateProfile, ServerMode, ServerProfile, SourceKind,
    SourceProfile, SourceSettings, StateProfile,
};
