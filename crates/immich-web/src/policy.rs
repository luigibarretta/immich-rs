use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_LENGTH, HOST, ORIGIN};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::WebConfig;

#[derive(Clone)]
pub struct RequestPolicy {
    allowed_host: Arc<str>,
    public_origin: Arc<str>,
    max_header_bytes: usize,
    max_body_bytes: usize,
    direct_tls_lan: bool,
}

impl RequestPolicy {
    pub fn new(config: &WebConfig) -> Self {
        Self {
            allowed_host: Arc::from(config.allowed_host()),
            public_origin: Arc::from(config.public_origin()),
            max_header_bytes: config.limits().request_header_bytes,
            max_body_bytes: config.limits().request_body_bytes,
            direct_tls_lan: config.lan().is_some(),
        }
    }
}

pub async fn request_policy(
    State(policy): State<RequestPolicy>,
    request: Request,
    next: Next,
) -> Response {
    let rejection = validate_request(&policy, &request);
    // Security response headers are applied by the outer `SecurityHeadersLayer`
    // (`security_headers.rs`), rejections included.
    match rejection {
        Some(status) => status.into_response(),
        None => next.run(request).await,
    }
}

fn validate_request(policy: &RequestPolicy, request: &Request) -> Option<StatusCode> {
    let headers = request.headers();
    if request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_none_or(|peer| !policy.direct_tls_lan && !peer.0.ip().is_loopback())
        || headers.get_all(HOST).iter().count() != 1
        || headers.get(HOST).and_then(|value| value.to_str().ok())
            != Some(policy.allowed_host.as_ref())
        || has_forwarded_header(headers)
        || header_bytes(headers) > policy.max_header_bytes
    {
        return Some(StatusCode::BAD_REQUEST);
    }
    if state_changing(request.method())
        && headers.get(ORIGIN).and_then(|value| value.to_str().ok())
            != Some(policy.public_origin.as_ref())
    {
        return Some(StatusCode::FORBIDDEN);
    }
    if headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > policy.max_body_bytes)
    {
        return Some(StatusCode::PAYLOAD_TOO_LARGE);
    }
    None
}

const fn state_changing(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn has_forwarded_header(headers: &HeaderMap) -> bool {
    [
        "forwarded",
        "x-forwarded-for",
        "x-forwarded-host",
        "x-forwarded-port",
        "x-forwarded-proto",
    ]
    .iter()
    .any(|name| headers.contains_key(*name))
}

fn header_bytes(headers: &HeaderMap) -> usize {
    headers.iter().fold(0_usize, |total, (name, value)| {
        total
            .saturating_add(name.as_str().len())
            .saturating_add(value.as_bytes().len())
            .saturating_add(4)
    })
}
