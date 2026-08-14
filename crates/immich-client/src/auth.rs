use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use reqwest::header::HeaderValue;

const MAX_API_KEY_BYTES: usize = 4_096;

/// Secret Immich API key with redacted debug output.
#[derive(Clone)]
pub struct ApiKey(HeaderValue);

impl ApiKey {
    /// Validate and protect an API key for authenticated requests.
    pub fn new(value: &str) -> Result<Self, ApiKeyError> {
        if value.is_empty() || value.len() > MAX_API_KEY_BYTES {
            return Err(ApiKeyError);
        }
        let mut header = HeaderValue::from_str(value).map_err(|_| ApiKeyError)?;
        header.set_sensitive(true);
        Ok(Self(header))
    }

    pub(crate) fn header(&self) -> HeaderValue {
        self.0.clone()
    }
}

impl Debug for ApiKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

/// An API key was empty, too large or invalid as an HTTP header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiKeyError;

impl Display for ApiKeyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid Immich API key")
    }
}

impl Error for ApiKeyError {}
