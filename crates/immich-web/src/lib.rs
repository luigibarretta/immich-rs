#![forbid(unsafe_code)]
//! Authenticated, bounded operator Web Console policy and HTTP surface.

mod auth;
#[cfg(test)]
mod auth_tests;
mod config;
mod error;
mod http;
mod policy;
mod profiles;
mod server;
mod views;

pub use config::{WebConfig, WebLimits};
pub use error::WebConfigError;
pub use http::WebConsole;
pub use profiles::{ResolvedSourceProfile, ServerProfile, SourceProfile};
