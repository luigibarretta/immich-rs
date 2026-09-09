use std::collections::BTreeSet;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use immich_rs_application::ImmichEndpoint;
use serde::Deserialize;
use url::{Position, Url};

use crate::error::WebConfigError;
use crate::profiles::{RawServerProfile, RawSourceProfile, ServerProfile, SourceProfile};

const CONFIG_SCHEMA_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_SOURCE_PROFILES: usize = 64;
const MAX_SERVER_PROFILES: usize = 32;

/// Validated Web Console resource bounds configurable only below hard maxima.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebLimits {
    /// Maximum aggregate request-header bytes after HTTP parsing.
    pub request_header_bytes: usize,
    /// Maximum request-body bytes enforced while streaming the body.
    pub request_body_bytes: usize,
    /// Maximum number of concurrently accepted loopback connections.
    pub accepted_connections: usize,
    /// Maximum duration allowed to read request headers.
    pub header_read_seconds: u64,
    /// Maximum duration of one non-streaming response.
    pub response_seconds: u64,
    /// Maximum in-memory authenticated sessions.
    pub max_sessions: usize,
    /// Idle session lifetime in seconds.
    pub session_idle_seconds: u64,
    /// Absolute session lifetime in seconds.
    pub session_absolute_seconds: u64,
    /// First-start bootstrap lifetime in seconds.
    pub bootstrap_lifetime_seconds: u64,
    /// Maximum concurrently running jobs.
    pub concurrent_jobs: usize,
    /// Maximum jobs waiting for a worker slot.
    pub queued_jobs: usize,
    /// Maximum in-memory job records, including terminal records.
    pub retained_jobs: usize,
    /// Maximum SSE subscribers owned by one session.
    pub sse_subscribers_per_session: usize,
    /// Maximum SSE subscribers in the process.
    pub sse_subscribers_per_process: usize,
    /// Maximum replay events retained by one job.
    pub sse_replay_events: usize,
    /// SSE heartbeat interval in seconds.
    pub sse_heartbeat_seconds: u64,
}

impl Default for WebLimits {
    fn default() -> Self {
        Self {
            request_header_bytes: 16 * 1_024,
            request_body_bytes: 16 * 1_024,
            accepted_connections: 16,
            header_read_seconds: 5,
            response_seconds: 15,
            max_sessions: 8,
            session_idle_seconds: 30 * 60,
            session_absolute_seconds: 8 * 60 * 60,
            bootstrap_lifetime_seconds: 10 * 60,
            concurrent_jobs: 1,
            queued_jobs: 4,
            retained_jobs: 128,
            sse_subscribers_per_session: 4,
            sse_subscribers_per_process: 16,
            sse_replay_events: 128,
            sse_heartbeat_seconds: 15,
        }
    }
}

impl WebLimits {
    fn validate(self) -> Result<Self, WebConfigError> {
        let valid = (1..=32 * 1_024).contains(&self.request_header_bytes)
            && (1..=64 * 1_024).contains(&self.request_body_bytes)
            && (1..=32).contains(&self.accepted_connections)
            && (1..=10).contains(&self.header_read_seconds)
            && (1..=30).contains(&self.response_seconds)
            && (1..=16).contains(&self.max_sessions)
            && (1..=60 * 60).contains(&self.session_idle_seconds)
            && (1..=12 * 60 * 60).contains(&self.session_absolute_seconds)
            && self.session_idle_seconds <= self.session_absolute_seconds
            && (1..=15 * 60).contains(&self.bootstrap_lifetime_seconds);
        let valid = valid
            && (1..=4).contains(&self.concurrent_jobs)
            && (1..=8).contains(&self.queued_jobs)
            && (1..=256).contains(&self.retained_jobs)
            && self.retained_jobs >= self.concurrent_jobs.saturating_add(self.queued_jobs)
            && (1..=8).contains(&self.sse_subscribers_per_session)
            && (1..=32).contains(&self.sse_subscribers_per_process)
            && self.sse_subscribers_per_session <= self.sse_subscribers_per_process
            && (1..=256).contains(&self.sse_replay_events)
            && (1..=30).contains(&self.sse_heartbeat_seconds);
        valid
            .then_some(self)
            .ok_or_else(|| WebConfigError::new("web resource limits are invalid"))
    }
}

/// Strict operator-owned configuration. Browser requests can reference only its IDs.
pub struct WebConfig {
    listen_address: SocketAddr,
    public_origin: String,
    allowed_host: String,
    bootstrap_secret_file: PathBuf,
    limits: WebLimits,
    sources: Vec<SourceProfile>,
    servers: Vec<ServerProfile>,
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
        Ok(Self {
            listen_address,
            public_origin,
            allowed_host,
            bootstrap_secret_file: raw.web.bootstrap_secret_file,
            limits,
            sources,
            servers,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: u32,
    web: RawWeb,
    sources: Vec<RawSourceProfile>,
    #[serde(default)]
    servers: Vec<RawServerProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWeb {
    listen_address: String,
    public_origin: String,
    bootstrap_secret_file: PathBuf,
    #[serde(default)]
    limits: RawWebLimits,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawWebLimits {
    request_header_bytes: Option<usize>,
    request_body_bytes: Option<usize>,
    accepted_connections: Option<usize>,
    header_read_seconds: Option<u64>,
    response_seconds: Option<u64>,
    max_sessions: Option<usize>,
    session_idle_seconds: Option<u64>,
    session_absolute_seconds: Option<u64>,
    bootstrap_lifetime_seconds: Option<u64>,
    concurrent_jobs: Option<usize>,
    queued_jobs: Option<usize>,
    retained_jobs: Option<usize>,
    sse_subscribers_per_session: Option<usize>,
    sse_subscribers_per_process: Option<usize>,
    sse_replay_events: Option<usize>,
    sse_heartbeat_seconds: Option<u64>,
}

impl RawWebLimits {
    fn into_limits(self) -> WebLimits {
        let defaults = WebLimits::default();
        WebLimits {
            request_header_bytes: self
                .request_header_bytes
                .unwrap_or(defaults.request_header_bytes),
            request_body_bytes: self
                .request_body_bytes
                .unwrap_or(defaults.request_body_bytes),
            accepted_connections: self
                .accepted_connections
                .unwrap_or(defaults.accepted_connections),
            header_read_seconds: self
                .header_read_seconds
                .unwrap_or(defaults.header_read_seconds),
            response_seconds: self.response_seconds.unwrap_or(defaults.response_seconds),
            max_sessions: self.max_sessions.unwrap_or(defaults.max_sessions),
            session_idle_seconds: self
                .session_idle_seconds
                .unwrap_or(defaults.session_idle_seconds),
            session_absolute_seconds: self
                .session_absolute_seconds
                .unwrap_or(defaults.session_absolute_seconds),
            bootstrap_lifetime_seconds: self
                .bootstrap_lifetime_seconds
                .unwrap_or(defaults.bootstrap_lifetime_seconds),
            concurrent_jobs: self.concurrent_jobs.unwrap_or(defaults.concurrent_jobs),
            queued_jobs: self.queued_jobs.unwrap_or(defaults.queued_jobs),
            retained_jobs: self.retained_jobs.unwrap_or(defaults.retained_jobs),
            sse_subscribers_per_session: self
                .sse_subscribers_per_session
                .unwrap_or(defaults.sse_subscribers_per_session),
            sse_subscribers_per_process: self
                .sse_subscribers_per_process
                .unwrap_or(defaults.sse_subscribers_per_process),
            sse_replay_events: self.sse_replay_events.unwrap_or(defaults.sse_replay_events),
            sse_heartbeat_seconds: self
                .sse_heartbeat_seconds
                .unwrap_or(defaults.sse_heartbeat_seconds),
        }
    }
}
