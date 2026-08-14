use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

/// Stable, privacy-safe class for one client failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientErrorClass {
    /// Authentication or authorization failed.
    Authentication,
    /// The server version is outside the supported compatibility range.
    Compatibility,
    /// The server requested throttling.
    RateLimited,
    /// A retryable server or request-timeout response was received.
    Server,
    /// The configured client timeout elapsed.
    Timeout,
    /// The transport disconnected before a definite response.
    Disconnect,
    /// A bounded response violated the API contract.
    Protocol,
    /// A local upload source could not be opened or read.
    Source,
    /// Cooperative cancellation stopped the request.
    Cancelled,
}

/// Privacy-safe client failure without response bodies, URLs or secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientError {
    class: ClientErrorClass,
    status: Option<u16>,
    retry_after: Option<Duration>,
}

impl ClientError {
    pub(crate) const fn new(class: ClientErrorClass) -> Self {
        Self {
            class,
            status: None,
            retry_after: None,
        }
    }

    pub(crate) const fn response(
        class: ClientErrorClass,
        status: u16,
        retry_after: Option<Duration>,
    ) -> Self {
        Self {
            class,
            status: Some(status),
            retry_after,
        }
    }

    /// Return the stable failure class.
    #[must_use]
    pub const fn class(self) -> ClientErrorClass {
        self.class
    }

    /// Return an HTTP status when a response was received.
    #[must_use]
    pub const fn status(self) -> Option<u16> {
        self.status
    }

    /// Return a bounded retry delay accepted from the server.
    #[must_use]
    pub const fn retry_after(self) -> Option<Duration> {
        self.retry_after
    }

    /// Return whether ADR-0019 permits retrying this class.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(
            self.class,
            ClientErrorClass::RateLimited
                | ClientErrorClass::Server
                | ClientErrorClass::Timeout
                | ClientErrorClass::Disconnect
        )
    }
}

impl Display for ClientError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "Immich request failed: {:?}", self.class)
    }
}

impl Error for ClientError {}
