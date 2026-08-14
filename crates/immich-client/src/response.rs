use std::time::Duration;

use reqwest::Response;
use reqwest::header::RETRY_AFTER;
use serde::de::DeserializeOwned;

use crate::{ClientError, ClientErrorClass};

pub async fn bounded_json<T: DeserializeOwned>(
    mut response: Response,
    max_bytes: usize,
    retry_after_cap: Duration,
) -> Result<T, ClientError> {
    let status = response.status();
    if !status.is_success() {
        return Err(status_error(&response, retry_after_cap));
    }
    let mut body = Vec::with_capacity(max_bytes.min(8 * 1_024));
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| classify_transport(&error))?
    {
        let next_len = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| ClientError::new(ClientErrorClass::Protocol))?;
        if next_len > max_bytes {
            return Err(ClientError::new(ClientErrorClass::Protocol));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| ClientError::new(ClientErrorClass::Protocol))
}

pub fn classify_transport(error: &reqwest::Error) -> ClientError {
    let class = if error.is_timeout() {
        ClientErrorClass::Timeout
    } else if error.is_connect() || error.is_request() || error.is_body() {
        ClientErrorClass::Disconnect
    } else {
        ClientErrorClass::Protocol
    };
    ClientError::new(class)
}

fn status_error(response: &Response, retry_after_cap: Duration) -> ClientError {
    let status = response.status().as_u16();
    let class = match status {
        401 | 403 => ClientErrorClass::Authentication,
        429 => ClientErrorClass::RateLimited,
        408 | 425 | 500..=599 => ClientErrorClass::Server,
        _ => ClientErrorClass::Protocol,
    };
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|delay| *delay <= retry_after_cap);
    ClientError::response(class, status, retry_after)
}
