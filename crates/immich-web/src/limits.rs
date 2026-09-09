use serde::Deserialize;

use crate::WebConfigError;

/// Validated Web Console resource bounds configurable only below hard maxima.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebLimits {
    /// Maximum aggregate request-header bytes after HTTP parsing.
    pub request_header_bytes: usize,
    /// Maximum request-body bytes enforced while streaming the body.
    pub request_body_bytes: usize,
    /// Maximum number of concurrently accepted console connections.
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
    /// Maximum unique addresses accepted from one DNS resolution.
    pub dns_addresses: usize,
    /// Maximum rows returned on one history page.
    pub history_page_rows: usize,
    /// Maximum terminal history rows retained.
    pub history_retained_rows: usize,
    /// Maximum age of terminal history and dry-run receipts.
    pub history_retention_days: u64,
    /// Maximum combined history database and WAL bytes.
    pub history_store_bytes: u64,
    /// Maximum immutable plan artifact bytes.
    pub plan_file_bytes: u64,
    /// Maximum aggregate immutable plan-store bytes.
    pub plan_store_bytes: u64,
    /// Maximum age of a dry-run receipt accepted for confirmation.
    pub dry_run_receipt_seconds: u64,
    /// Maximum monotonic lifetime of one in-memory production grant.
    pub production_grant_seconds: u64,
    /// Maximum UTF-8 bytes accepted for a backup reference.
    pub backup_reference_bytes: usize,
    /// Maximum bounded OIDC discovery response bytes.
    pub oidc_discovery_bytes: usize,
    /// Maximum bounded OIDC JWKS response bytes.
    pub oidc_jwks_bytes: usize,
    /// Maximum OIDC signing keys accepted from one JWKS.
    pub oidc_signing_keys: usize,
    /// Maximum age of one single-use OIDC state and nonce.
    pub oidc_state_seconds: u64,
    /// Maximum token endpoint response and ID token bytes.
    pub oidc_token_bytes: usize,
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
            dns_addresses: 4,
            history_page_rows: 50,
            history_retained_rows: 5_000,
            history_retention_days: 30,
            history_store_bytes: 32 * 1_024 * 1_024,
            plan_file_bytes: 128 * 1_024 * 1_024,
            plan_store_bytes: 512 * 1_024 * 1_024,
            dry_run_receipt_seconds: 5 * 60,
            production_grant_seconds: 90,
            backup_reference_bytes: 256,
            oidc_discovery_bytes: 64 * 1_024,
            oidc_jwks_bytes: 256 * 1_024,
            oidc_signing_keys: 8,
            oidc_state_seconds: 3 * 60,
            oidc_token_bytes: 32 * 1_024,
        }
    }
}

impl WebLimits {
    pub(crate) fn validate(self) -> Result<Self, WebConfigError> {
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
            && (1..=30).contains(&self.sse_heartbeat_seconds)
            && (1..=8).contains(&self.dns_addresses)
            && (1..=100).contains(&self.history_page_rows)
            && (1..=10_000).contains(&self.history_retained_rows)
            && self.history_page_rows <= self.history_retained_rows
            && (1..=90).contains(&self.history_retention_days)
            && (1_024 * 1_024..=64 * 1_024 * 1_024).contains(&self.history_store_bytes)
            && (1_024 * 1_024..=256 * 1_024 * 1_024).contains(&self.plan_file_bytes);
        let valid = valid
            && self.plan_store_bytes >= self.plan_file_bytes.saturating_add(16 * 1_024)
            && self.plan_store_bytes <= 4 * 1_024 * 1_024 * 1_024
            && (1..=10 * 60).contains(&self.dry_run_receipt_seconds)
            && (1..=120).contains(&self.production_grant_seconds)
            && (1..=256).contains(&self.backup_reference_bytes);
        let valid = valid
            && (1..=128 * 1_024).contains(&self.oidc_discovery_bytes)
            && (1..=512 * 1_024).contains(&self.oidc_jwks_bytes)
            && (1..=16).contains(&self.oidc_signing_keys)
            && (1..=5 * 60).contains(&self.oidc_state_seconds)
            && (1..=64 * 1_024).contains(&self.oidc_token_bytes);
        valid
            .then_some(self)
            .ok_or_else(|| WebConfigError::new("web resource limits are invalid"))
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawWebLimits {
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
    dns_addresses: Option<usize>,
    history_page_rows: Option<usize>,
    history_retained_rows: Option<usize>,
    history_retention_days: Option<u64>,
    history_store_bytes: Option<u64>,
    plan_file_bytes: Option<u64>,
    plan_store_bytes: Option<u64>,
    dry_run_receipt_seconds: Option<u64>,
    production_grant_seconds: Option<u64>,
    backup_reference_bytes: Option<usize>,
    oidc_discovery_bytes: Option<usize>,
    oidc_jwks_bytes: Option<usize>,
    oidc_signing_keys: Option<usize>,
    oidc_state_seconds: Option<u64>,
    oidc_token_bytes: Option<usize>,
}

impl RawWebLimits {
    pub fn into_limits(self) -> WebLimits {
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
            dns_addresses: self.dns_addresses.unwrap_or(defaults.dns_addresses),
            history_page_rows: self.history_page_rows.unwrap_or(defaults.history_page_rows),
            history_retained_rows: self
                .history_retained_rows
                .unwrap_or(defaults.history_retained_rows),
            history_retention_days: self
                .history_retention_days
                .unwrap_or(defaults.history_retention_days),
            history_store_bytes: self
                .history_store_bytes
                .unwrap_or(defaults.history_store_bytes),
            plan_file_bytes: self.plan_file_bytes.unwrap_or(defaults.plan_file_bytes),
            plan_store_bytes: self.plan_store_bytes.unwrap_or(defaults.plan_store_bytes),
            dry_run_receipt_seconds: self
                .dry_run_receipt_seconds
                .unwrap_or(defaults.dry_run_receipt_seconds),
            production_grant_seconds: self
                .production_grant_seconds
                .unwrap_or(defaults.production_grant_seconds),
            backup_reference_bytes: self
                .backup_reference_bytes
                .unwrap_or(defaults.backup_reference_bytes),
            oidc_discovery_bytes: self
                .oidc_discovery_bytes
                .unwrap_or(defaults.oidc_discovery_bytes),
            oidc_jwks_bytes: self.oidc_jwks_bytes.unwrap_or(defaults.oidc_jwks_bytes),
            oidc_signing_keys: self.oidc_signing_keys.unwrap_or(defaults.oidc_signing_keys),
            oidc_state_seconds: self
                .oidc_state_seconds
                .unwrap_or(defaults.oidc_state_seconds),
            oidc_token_bytes: self.oidc_token_bytes.unwrap_or(defaults.oidc_token_bytes),
        }
    }
}
