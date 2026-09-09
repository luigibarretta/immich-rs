use std::collections::BTreeSet;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use immich_rs_application::ImmichEndpoint;
use serde::Deserialize;
use url::{Position, Url};

use crate::error::WebConfigError;
use crate::limits::{RawWebLimits, WebLimits};
use crate::profiles::{
    RawServerProfile, RawSourceProfile, RawStateProfile, ServerProfile, SourceProfile, StateProfile,
};

const CONFIG_SCHEMA_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_SOURCE_PROFILES: usize = 64;
const MAX_SERVER_PROFILES: usize = 32;
const MAX_STATE_PROFILES: usize = 32;

/// Strict operator-owned configuration. Browser requests can reference only its IDs.
pub struct WebConfig {
    listen_address: SocketAddr,
    public_origin: String,
    allowed_host: String,
    bootstrap_secret_file: PathBuf,
    limits: WebLimits,
    sources: Vec<SourceProfile>,
    servers: Vec<ServerProfile>,
    states: Vec<StateProfile>,
    history_state_id: String,
}

impl WebConfig {
    /// Load one bounded, regular, non-symlink configuration file.
    pub fn load(path: &Path) -> Result<Self, WebConfigError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| WebConfigError::new("cannot read web configuration"))?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() == 0
            || metadata.len() > MAX_CONFIG_BYTES
        {
            return Err(WebConfigError::new(
                "web configuration must be a bounded regular file",
            ));
        }
        let contents = fs::read_to_string(path)
            .map_err(|_| WebConfigError::new("cannot read web configuration"))?;
        let raw: RawConfig = toml::from_str(&contents)
            .map_err(|_| WebConfigError::new("invalid web configuration"))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, WebConfigError> {
        if raw.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(WebConfigError::new("unsupported web configuration schema"));
        }
        let listen_address = raw
            .web
            .listen_address
            .parse::<SocketAddr>()
            .map_err(|_| WebConfigError::new("invalid web listen address"))?;
        if !listen_address.ip().is_loopback() {
            return Err(WebConfigError::new(
                "loopback mode requires a loopback listen address",
            ));
        }
        let (public_origin, allowed_host) =
            validate_public_origin(&raw.web.public_origin, listen_address.port())?;
        if !raw.web.bootstrap_secret_file.is_absolute() {
            return Err(WebConfigError::new(
                "bootstrap secret file path must be absolute",
            ));
        }
        let limits = raw.web.limits.into_limits().validate()?;
        let sources = validate_sources(raw.sources)?;
        let servers = validate_servers(raw.servers)?;
        let states = validate_states(raw.states)?;
        if !states
            .iter()
            .any(|profile| profile.id() == raw.web.history_state_id)
        {
            return Err(WebConfigError::new("history state profile is unknown"));
        }
        Ok(Self {
            listen_address,
            public_origin,
            allowed_host,
            bootstrap_secret_file: raw.web.bootstrap_secret_file,
            limits,
            sources,
            servers,
            states,
            history_state_id: raw.web.history_state_id,
        })
    }

    /// Loopback socket accepted by this first console slice.
    #[must_use]
    pub const fn listen_address(&self) -> SocketAddr {
        self.listen_address
    }

    /// Exact scheme and authority required by Origin checks.
    #[must_use]
    pub fn public_origin(&self) -> &str {
        &self.public_origin
    }

    /// Exact Host header allowlist entry.
    #[must_use]
    pub fn allowed_host(&self) -> &str {
        &self.allowed_host
    }

    /// Operator-configured bootstrap secret file, never supplied by a browser.
    #[must_use]
    pub fn bootstrap_secret_file(&self) -> &Path {
        &self.bootstrap_secret_file
    }

    /// Validated session and bootstrap limits.
    #[must_use]
    pub const fn limits(&self) -> WebLimits {
        self.limits
    }

    /// Resolve one opaque source ID without accepting a browser path.
    #[must_use]
    pub fn source(&self, id: &str) -> Option<&SourceProfile> {
        self.sources.iter().find(|profile| profile.id() == id)
    }

    /// Configured source profiles in stable operator order.
    #[must_use]
    pub fn sources(&self) -> &[SourceProfile] {
        &self.sources
    }

    /// Resolve one opaque server ID without accepting a browser origin.
    #[must_use]
    pub fn server(&self, id: &str) -> Option<&ServerProfile> {
        self.servers.iter().find(|profile| profile.id() == id)
    }

    /// Configured server profiles in stable operator order.
    #[must_use]
    pub fn servers(&self) -> &[ServerProfile] {
        &self.servers
    }

    /// Resolve one opaque private state profile ID.
    #[must_use]
    pub fn state(&self, id: &str) -> Option<&StateProfile> {
        self.states.iter().find(|profile| profile.id() == id)
    }

    /// Return the operator-selected state profile for console history.
    pub fn history_state(&self) -> Result<&StateProfile, WebConfigError> {
        self.state(&self.history_state_id)
            .ok_or_else(|| WebConfigError::new("history state profile is unavailable"))
    }
}

fn validate_public_origin(
    value: &str,
    listen_port: u16,
) -> Result<(String, String), WebConfigError> {
    let url = Url::parse(value).map_err(|_| WebConfigError::new("invalid public origin"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.scheme() != "http"
        || url.port_or_known_default() != Some(listen_port)
    {
        return Err(WebConfigError::new("invalid loopback public origin"));
    }
    let endpoint = ImmichEndpoint::parse(value)
        .map_err(|_| WebConfigError::new("invalid loopback public origin"))?;
    if !endpoint.is_loopback() {
        return Err(WebConfigError::new("public origin must be loopback"));
    }
    let authority = url[Position::BeforeHost..Position::AfterPort].to_owned();
    Ok((format!("{}://{authority}", url.scheme()), authority))
}

fn validate_sources(raw: Vec<RawSourceProfile>) -> Result<Vec<SourceProfile>, WebConfigError> {
    if raw.is_empty() || raw.len() > MAX_SOURCE_PROFILES {
        return Err(WebConfigError::new("source profile count is invalid"));
    }
    let mut ids = BTreeSet::new();
    raw.into_iter()
        .map(|profile| {
            if !ids.insert(profile.id.clone()) {
                return Err(WebConfigError::new("profile identifiers must be unique"));
            }
            SourceProfile::from_raw(profile)
        })
        .collect()
}

fn validate_servers(raw: Vec<RawServerProfile>) -> Result<Vec<ServerProfile>, WebConfigError> {
    if raw.len() > MAX_SERVER_PROFILES {
        return Err(WebConfigError::new("server profile count is invalid"));
    }
    let mut ids = BTreeSet::new();
    raw.into_iter()
        .map(|profile| {
            if !ids.insert(profile.id.clone()) {
                return Err(WebConfigError::new("profile identifiers must be unique"));
            }
            ServerProfile::from_raw(profile)
        })
        .collect()
}

fn validate_states(raw: Vec<RawStateProfile>) -> Result<Vec<StateProfile>, WebConfigError> {
    if raw.is_empty() || raw.len() > MAX_STATE_PROFILES {
        return Err(WebConfigError::new("state profile count is invalid"));
    }
    let mut ids = BTreeSet::new();
    raw.into_iter()
        .map(|profile| {
            if !ids.insert(profile.id.clone()) {
                return Err(WebConfigError::new("profile identifiers must be unique"));
            }
            StateProfile::from_raw(profile)
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: u32,
    web: RawWeb,
    sources: Vec<RawSourceProfile>,
    #[serde(default)]
    servers: Vec<RawServerProfile>,
    states: Vec<RawStateProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWeb {
    listen_address: String,
    public_origin: String,
    bootstrap_secret_file: PathBuf,
    history_state_id: String,
    #[serde(default)]
    limits: RawWebLimits,
}
