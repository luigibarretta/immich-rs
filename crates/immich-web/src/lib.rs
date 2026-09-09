#![forbid(unsafe_code)]
//! Authenticated, bounded operator Web Console policy and HTTP surface.

mod config;
mod error;
mod profiles;

pub use config::{WebConfig, WebLimits};
pub use error::WebConfigError;
pub use profiles::{ResolvedSourceProfile, ServerProfile, SourceProfile};
