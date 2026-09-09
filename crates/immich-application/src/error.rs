use std::error::Error;
use std::fmt::{self, Display, Formatter};

use immich_rs_client::{ClientError, ClientErrorClass};
use immich_rs_executor::{ExecutorError, ExecutorErrorClass};
use immich_rs_sources::ScanError;

/// Frontend-neutral error class preserving the established CLI exit mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationErrorClass {
    /// Invalid request or unsupported capability.
    Usage,
    /// Local source failure.
    Source,
    /// Credential or server authentication failure.
    Authentication,
    /// Unsupported server compatibility.
    Compatibility,
    /// Remote transport failure.
    Network,
    /// Durable checkpoint failure.
    Checkpoint,
    /// Local destination failure.
    Destination,
    /// Internal invariant or protocol failure.
    Invariant,
    /// Cooperative cancellation.
    Cancelled,
}

/// Typed error returned by application workflow composition.
#[derive(Debug)]
pub enum ApplicationError {
    /// Source adapter failure.
    Scan(ScanError),
    /// Immich client failure.
    Client(ClientError),
    /// Executor failure.
    Executor(ExecutorError),
    /// Frontend-independent invalid request.
    InvalidRequest(&'static str),
}

impl ApplicationError {
    /// Stable class used by frontend presentation and exit policy.
    #[must_use]
    pub const fn class(&self) -> ApplicationErrorClass {
        match self {
            Self::Scan(ScanError::Cancelled) => ApplicationErrorClass::Cancelled,
            Self::Scan(ScanError::InvalidPlan(_)) => ApplicationErrorClass::Invariant,
            Self::Scan(_) => ApplicationErrorClass::Source,
            Self::Client(error) => match error.class() {
                ClientErrorClass::Authentication => ApplicationErrorClass::Authentication,
                ClientErrorClass::Compatibility => ApplicationErrorClass::Compatibility,
                ClientErrorClass::Cancelled => ApplicationErrorClass::Cancelled,
                ClientErrorClass::Protocol => ApplicationErrorClass::Invariant,
                _ => ApplicationErrorClass::Network,
            },
            Self::Executor(error) => match error.class() {
                ExecutorErrorClass::Cancelled => ApplicationErrorClass::Cancelled,
                ExecutorErrorClass::Checkpoint => ApplicationErrorClass::Checkpoint,
                ExecutorErrorClass::Destination => ApplicationErrorClass::Destination,
                ExecutorErrorClass::SourceChanged
                | ExecutorErrorClass::SourceDiagnostics
                | ExecutorErrorClass::UnsupportedMetadata => ApplicationErrorClass::Source,
                ExecutorErrorClass::Client => ApplicationErrorClass::Network,
                _ => ApplicationErrorClass::Invariant,
            },
            Self::InvalidRequest(_) => ApplicationErrorClass::Usage,
        }
    }
}

impl Display for ApplicationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan(error) => Display::fmt(error, formatter),
            Self::Client(error) => Display::fmt(error, formatter),
            Self::Executor(error) => Display::fmt(error, formatter),
            Self::InvalidRequest(message) => formatter.write_str(message),
        }
    }
}

impl Error for ApplicationError {}

impl From<ScanError> for ApplicationError {
    fn from(error: ScanError) -> Self {
        Self::Scan(error)
    }
}

impl From<ClientError> for ApplicationError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

impl From<ExecutorError> for ApplicationError {
    fn from(error: ExecutorError) -> Self {
        Self::Executor(error)
    }
}
