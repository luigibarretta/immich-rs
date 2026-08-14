use std::fmt::{self, Debug, Formatter};
use std::time::Duration;

use immich_rs_core::{Cancellation, ServerCompatibility, ServerVersion};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};

use crate::models::{UserResponse, VersionResponse};
use crate::response::{bounded_json, classify_transport};
use crate::{ApiKey, ClientError, ClientErrorClass, ImmichEndpoint, ImmichUploadClient};

const SUPPORTED_MAJOR: u32 = 3;
const SUPPORTED_MINOR: u32 = 1;

/// Bounded network limits shared by read and upload capabilities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientConfig {
    /// Total timeout for one request.
    pub request_timeout: Duration,
    /// Maximum accepted JSON response body.
    pub max_response_bytes: usize,
    /// Maximum accepted `Retry-After` delay.
    pub retry_after_cap: Duration,
    /// Per-file streaming buffer.
    pub upload_buffer_bytes: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_response_bytes: 64 * 1_024,
            retry_after_cap: Duration::from_secs(60),
            upload_buffer_bytes: 64 * 1_024,
        }
    }
}

impl ClientConfig {
    pub(crate) fn validate(self) -> Result<Self, ClientError> {
        let valid = !self.request_timeout.is_zero()
            && (1_024..=1024 * 1_024).contains(&self.max_response_bytes)
            && (4 * 1_024..=4 * 1_024 * 1_024).contains(&self.upload_buffer_bytes)
            && self.retry_after_cap <= Duration::from_secs(300);
        valid
            .then_some(self)
            .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))
    }
}

/// Read-only authenticated Immich capability.
#[derive(Clone)]
pub struct ImmichReadClient {
    pub(crate) http: reqwest::Client,
    pub(crate) endpoint: ImmichEndpoint,
    pub(crate) api_key: ApiKey,
    pub(crate) config: ClientConfig,
}

/// Opaque proof of a successful authenticated compatibility probe.
#[derive(Clone)]
pub struct NegotiatedServer {
    compatibility: ServerCompatibility,
    origin_sha256: String,
}

impl NegotiatedServer {
    /// Return privacy-safe facts suitable for an immutable upload plan.
    #[must_use]
    pub const fn compatibility(&self) -> &ServerCompatibility {
        &self.compatibility
    }
}

impl Debug for NegotiatedServer {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NegotiatedServer")
            .field("compatibility", &"[REDACTED]")
            .field("origin", &"[REDACTED]")
            .finish()
    }
}

impl ImmichReadClient {
    /// Construct a read-only client. No network request is made.
    pub fn new(
        endpoint: ImmichEndpoint,
        api_key: ApiKey,
        config: ClientConfig,
    ) -> Result<Self, ClientError> {
        let config = config.validate()?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(config.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
        Ok(Self {
            http,
            endpoint,
            api_key,
            config,
        })
    }

    /// Probe exact server compatibility and authenticated identity read-only.
    pub async fn probe(
        &self,
        cancellation: &impl Cancellation,
    ) -> Result<NegotiatedServer, ClientError> {
        self.endpoint
            .require_phase_two_loopback()
            .map_err(|_| ClientError::new(ClientErrorClass::Compatibility))?;
        check_cancelled(cancellation)?;
        let version: VersionResponse = self.get_json("api/server/version").await?;
        if version.major != SUPPORTED_MAJOR
            || version.minor != SUPPORTED_MINOR
            || version.prerelease.is_some()
        {
            return Err(ClientError::new(ClientErrorClass::Compatibility));
        }
        check_cancelled(cancellation)?;
        let user: UserResponse = self.get_json("api/users/me").await?;
        if user.id.is_empty() || user.id.len() > 4_096 {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        let identity_sha256 = identity_digest(self.endpoint.canonical_origin(), &user.id);
        let compatibility = ServerCompatibility {
            version: ServerVersion {
                major: version.major,
                minor: version.minor,
                patch: version.patch,
            },
            identity_sha256,
        };
        Ok(NegotiatedServer {
            compatibility,
            origin_sha256: format!(
                "{:x}",
                Sha256::digest(self.endpoint.canonical_origin().as_bytes())
            ),
        })
    }

    /// Consume this read client and a matching probe proof to enable uploads.
    pub fn authorize_upload(
        self,
        negotiated: NegotiatedServer,
    ) -> Result<ImmichUploadClient, ClientError> {
        let origin_sha256 = format!(
            "{:x}",
            Sha256::digest(self.endpoint.canonical_origin().as_bytes())
        );
        if origin_sha256 != negotiated.origin_sha256 {
            return Err(ClientError::new(ClientErrorClass::Compatibility));
        }
        Ok(ImmichUploadClient::from_negotiated(
            self,
            negotiated.compatibility,
        ))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let url = self
            .endpoint
            .api_url(path)
            .map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
        let response = self
            .http
            .get(url)
            .header("x-api-key", self.api_key.header())
            .send()
            .await
            .map_err(|error| classify_transport(&error))?;
        bounded_json(
            response,
            self.config.max_response_bytes,
            self.config.retry_after_cap,
        )
        .await
    }
}

impl Debug for ImmichReadClient {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmichReadClient")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

fn identity_digest(origin: &str, user_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(origin.as_bytes());
    hasher.update([0]);
    hasher.update(user_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn check_cancelled(cancellation: &impl Cancellation) -> Result<(), ClientError> {
    if cancellation.is_cancelled() {
        Err(ClientError::new(ClientErrorClass::Cancelled))
    } else {
        Ok(())
    }
}
