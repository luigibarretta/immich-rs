use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Stable, privacy-safe executor failure class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorErrorClass {
    /// A resource or retry limit is invalid.
    InvalidConfiguration,
    /// A plan schema or invariant failed.
    InvalidPlan,
    /// Source diagnostics prevent safe apply.
    SourceDiagnostics,
    /// Unsupported or ambiguous metadata was planned.
    UnsupportedMetadata,
    /// Source content, timestamp or native type changed.
    SourceChanged,
    /// Cooperative cancellation stopped verification.
    Cancelled,
    /// A checkpoint is invalid, corrupt or bound to other inputs.
    Checkpoint,
    /// The local archive destination is unsafe or inconsistent.
    Destination,
    /// An Immich request failed definitively.
    Client,
    /// Serialization or an internal invariant failed closed.
    Invariant,
}

/// Privacy-safe executor error without paths, digests, IDs or secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutorError {
    class: ExecutorErrorClass,
}

impl ExecutorError {
    pub(crate) const fn new(class: ExecutorErrorClass) -> Self {
        Self { class }
    }

    /// Return the stable failure class.
    #[must_use]
    pub const fn class(self) -> ExecutorErrorClass {
        self.class
    }
}

impl Display for ExecutorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "execution failed: {:?}", self.class)
    }
}

impl Error for ExecutorError {}
