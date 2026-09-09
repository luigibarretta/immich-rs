use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, HOST, ORIGIN, PRAGMA, REFERRER_POLICY,
    X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::WebConfig;

#[derive(Clone)]
pub struct RequestPolicy {
    allowed_host: Arc<str>,
    public_origin: Arc<str>,
    max_header_bytes: usize,
    max_body_bytes: usize,
}

impl RequestPolicy {
    pub fn new(config: &WebConfig) -> Self {
        Self {
            allowed_host: Arc::from(config.allowed_host()),
            public_origin: Arc::from(config.public_origin()),
            max_header_bytes: config.limits().request_header_bytes,
            max_body_bytes: config.limits().request_body_bytes,
        }
    }
}

pub async fn request_policy(
    State(policy): State<RequestPolicy>,
    request: Request,
    next: Next,
) -> Response {
    let rejection = validate_request(&policy, &request);
    let mut response = match rejection {
        Some(status) => status.into_response(),
        None => next.run(request).await,
    };
    harden_response(response.headers_mut());
    response
}

fn validate_request(policy: &RequestPolicy, request: &Request) -> Option<StatusCode> {
    let headers = request.headers();
    if request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_none_or(|peer| !peer.0.ip().is_loopback())
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

fn harden_response(headers: &mut HeaderMap) {
    const VALUES: [(HeaderName, &str); 10] = [
        (CACHE_CONTROL, "private, no-store, max-age=0"),
        (PRAGMA, "no-cache"),
        (
            CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'self'; script-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
        (X_FRAME_OPTIONS, "DENY"),
        (REFERRER_POLICY, "no-referrer"),
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (
            HeaderName::from_static("permissions-policy"),
            "camera=(), microphone=(), geolocation=()",
        ),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
        (
            HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
        (HeaderName::from_static("x-robots-tag"), "noindex, nofollow"),
    ];
    for (name, value) in VALUES {
        headers.insert(name, HeaderValue::from_static(value));
    }
}
