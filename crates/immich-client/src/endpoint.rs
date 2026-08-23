use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use url::{Host, Url};

/// Validated Immich origin without credentials or path state.
#[derive(Clone)]
pub struct ImmichEndpoint {
    origin: Url,
    loopback: bool,
}

/// Validated network scope for one authenticated read client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointAccess {
    mode: AccessMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccessMode {
    Disposable,
    ProductionRead,
}

impl EndpointAccess {
    /// Authorize only a literal loopback endpoint for disposable testing.
    pub fn disposable(endpoint: &ImmichEndpoint) -> Result<Self, EndpointError> {
        endpoint
            .loopback
            .then_some(Self {
                mode: AccessMode::Disposable,
            })
            .ok_or(EndpointError::DisposableRequiresLoopback)
    }

    /// Authorize a remote HTTPS endpoint for an explicit read-only operation.
    pub fn production_read(
        endpoint: &ImmichEndpoint,
        acknowledged: bool,
    ) -> Result<Self, EndpointError> {
        if !acknowledged {
            return Err(EndpointError::ProductionReadNotAcknowledged);
        }
        if endpoint.loopback || endpoint.origin.scheme() != "https" {
            return Err(EndpointError::ProductionReadRequiresRemoteHttps);
        }
        Ok(Self {
            mode: AccessMode::ProductionRead,
        })
    }

    pub(crate) const fn permits_upload(self) -> bool {
        matches!(self.mode, AccessMode::Disposable)
    }

    pub(crate) const fn permits_production_upload(self) -> bool {
        matches!(self.mode, AccessMode::ProductionRead)
    }
}

impl ImmichEndpoint {
    /// Parse an HTTPS origin, or an HTTP origin only when it is loopback.
    pub fn parse(value: &str) -> Result<Self, EndpointError> {
        let mut origin = Url::parse(value).map_err(|_| EndpointError::InvalidOrigin)?;
        if !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !matches!(origin.path(), "" | "/")
        {
            return Err(EndpointError::InvalidOrigin);
        }
        let loopback = origin.host().as_ref().is_some_and(is_loopback_host);
        match origin.scheme() {
            "https" => {}
            "http" if loopback => {}
            "http" => return Err(EndpointError::InsecureRemoteOrigin),
            _ => return Err(EndpointError::UnsupportedScheme),
        }
        origin.set_path("/");
        Ok(Self { origin, loopback })
    }

    /// Return whether the endpoint is a literal loopback address or localhost.
    #[must_use]
    pub const fn is_loopback(&self) -> bool {
        self.loopback
    }

    pub(crate) fn api_url(&self, path: &str) -> Result<Url, EndpointError> {
        self.origin
            .join(path)
            .map_err(|_| EndpointError::InvalidOrigin)
    }

    pub(crate) fn canonical_origin(&self) -> &str {
        self.origin.as_str()
    }
}

impl Debug for ImmichEndpoint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmichEndpoint")
            .field("origin", &"[REDACTED]")
            .field("loopback", &self.loopback)
            .finish()
    }
}

fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    }
}

/// Reason an Immich origin was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointError {
    /// The value was not a path-free URL origin.
    InvalidOrigin,
    /// Only HTTP and HTTPS are supported.
    UnsupportedScheme,
    /// Plain HTTP is permitted only on loopback.
    InsecureRemoteOrigin,
    /// Disposable mode requires a literal loopback origin.
    DisposableRequiresLoopback,
    /// Remote read access was not explicitly acknowledged.
    ProductionReadNotAcknowledged,
    /// Production read mode requires a non-loopback HTTPS origin.
    ProductionReadRequiresRemoteHttps,
}

impl Display for EndpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidOrigin => "endpoint must be a credential-free URL origin",
            Self::UnsupportedScheme => "endpoint scheme must be HTTP or HTTPS",
            Self::InsecureRemoteOrigin => "plain HTTP requires a loopback endpoint",
            Self::DisposableRequiresLoopback => "disposable mode requires a loopback endpoint",
            Self::ProductionReadNotAcknowledged => {
                "production read access requires an explicit acknowledgement"
            }
            Self::ProductionReadRequiresRemoteHttps => {
                "production read access requires a remote HTTPS endpoint"
            }
        };
        formatter.write_str(message)
    }
}

impl Error for EndpointError {}
