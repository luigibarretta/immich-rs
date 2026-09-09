use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Path-redacted configuration or configured-resource failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebConfigError {
    message: &'static str,
}

impl WebConfigError {
    pub(crate) const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl Display for WebConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for WebConfigError {}
