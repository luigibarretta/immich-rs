use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use url::{Host, Url};

/// Validated Immich origin without credentials or path state.
#[derive(Clone)]
pub struct ImmichEndpoint {
    origin: Url,
    loopback: bool,
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

    pub(crate) fn require_phase_two_loopback(&self) -> Result<(), EndpointError> {
        self.loopback
            .then_some(())
            .ok_or(EndpointError::PhaseTwoRequiresLoopback)
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
    /// Phase 2 intentionally permits only disposable loopback servers.
    PhaseTwoRequiresLoopback,
}

impl Display for EndpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidOrigin => "endpoint must be a credential-free URL origin",
            Self::UnsupportedScheme => "endpoint scheme must be HTTP or HTTPS",
            Self::InsecureRemoteOrigin => "plain HTTP requires a loopback endpoint",
            Self::PhaseTwoRequiresLoopback => "phase two requires a loopback endpoint",
        };
        formatter.write_str(message)
    }
}

impl Error for EndpointError {}
