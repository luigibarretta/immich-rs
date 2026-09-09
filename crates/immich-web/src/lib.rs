#![forbid(unsafe_code)]
//! Authenticated, bounded operator Web Console policy and HTTP surface.

mod auth;
#[cfg(test)]
mod auth_tests;
mod config;
mod cookies;
mod error;
mod events_http;
mod http;
mod job_http;
mod jobs;
#[cfg(test)]
mod jobs_tests;
mod limits;
mod policy;
mod profiles;
mod server;
mod views;

pub use config::WebConfig;
pub use error::WebConfigError;
pub use http::WebConsole;
pub use limits::WebLimits;
pub use profiles::{
    ResolvedSourceProfile, ResolvedStateProfile, ServerMode, ServerProfile, SourceProfile,
    StateProfile,
};
