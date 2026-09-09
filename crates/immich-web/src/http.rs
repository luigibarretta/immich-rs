use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, Form, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use crate::auth::{AuthStore, PairingFailure, PairingView, SessionView};
use crate::policy::{RequestPolicy, request_policy};
use crate::{WebConfig, WebConfigError, views};

const SESSION_COOKIE: &str = "immich_rs_session";
const PAIRING_COOKIE: &str = "immich_rs_pairing";
const STYLESHEET: &str = include_str!("../assets/console.css");

/// Authenticated loopback console assembled from validated operator configuration.
pub struct WebConsole {
    state: ConsoleState,
}

#[derive(Clone)]
struct ConsoleState {
    config: Arc<WebConfig>,
    auth: Arc<AuthStore>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairForm {
    csrf: String,
    secret: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsrfForm {
    csrf: String,
}

impl WebConsole {
    /// Load the private bootstrap secret and initialize restart-ephemeral auth state.
    pub fn from_config(config: WebConfig) -> Result<Self, WebConfigError> {
        let auth = AuthStore::load(config.bootstrap_secret_file(), config.limits())?;
        Ok(Self {
            state: ConsoleState {
                config: Arc::new(config),
                auth: Arc::new(auth),
            },
        })
    }

    /// Build the bounded HTTP router. No filesystem or server operation is routed yet.
    pub fn router(&self) -> Router {
        let limits = self.state.config.limits();
        let policy = RequestPolicy::new(&self.state.config);
        Router::new()
            .route("/", get(index))
            .route("/pair", get(pair_page).post(pair))
            .route("/logout", post(logout))
            .route("/assets/console.css", get(stylesheet))
            .fallback(not_found)
            .layer(RequestBodyLimitLayer::new(limits.request_body_bytes))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(limits.response_seconds),
            ))
            .layer(middleware::from_fn_with_state(policy, request_policy))
            .with_state(self.state.clone())
    }
}

async fn index(State(state): State<ConsoleState>, headers: HeaderMap) -> Response {
    if let Some(session) = authenticated_session(&state, &headers) {
        return views::dashboard(&session.csrf_token, state.config.sources());
    }
    pairing_response(&state, false)
}

async fn pair_page(State(state): State<ConsoleState>, headers: HeaderMap) -> Response {
    if authenticated_session(&state, &headers).is_some() {
        return Redirect::to("/").into_response();
    }
    pairing_response(&state, false)
}

async fn pair(
    State(state): State<ConsoleState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PairForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, PAIRING_COOKIE) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    match state
        .auth
        .pair(peer.ip(), &cookie_token, &form.csrf, &form.secret)
    {
        Ok(session) => {
            let mut response = Redirect::to("/").into_response();
            if append_cookie(
                &mut response,
                SESSION_COOKIE,
                &session.cookie_token,
                state.config.limits().session_absolute_seconds,
            ) && append_clear_cookie(&mut response, PAIRING_COOKIE)
            {
                response
            } else {
                views::locked(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
        Err(PairingFailure::Denied) => pairing_response(&state, true),
        Err(PairingFailure::InvalidCsrf) => views::locked(StatusCode::FORBIDDEN),
        Err(PairingFailure::RateLimited) => {
            let mut response = views::locked(StatusCode::TOO_MANY_REQUESTS);
            response
                .headers_mut()
                .insert("retry-after", HeaderValue::from_static("300"));
            response
        }
        Err(PairingFailure::Expired) => views::locked(StatusCode::UNAUTHORIZED),
        Err(PairingFailure::Unavailable) => views::locked(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn logout(
    State(state): State<ConsoleState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    if !state.auth.logout(&cookie_token, &form.csrf) {
        return views::locked(StatusCode::FORBIDDEN);
    }
    let mut response = Redirect::to("/").into_response();
    if append_clear_cookie(&mut response, SESSION_COOKIE) {
        response
    } else {
        views::locked(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

async fn stylesheet() -> Response {
    ([(CONTENT_TYPE, "text/css; charset=utf-8")], STYLESHEET).into_response()
}

async fn not_found() -> Response {
    views::locked(StatusCode::NOT_FOUND)
}

fn authenticated_session(state: &ConsoleState, headers: &HeaderMap) -> Option<SessionView> {
    let cookie_token = cookie_value(headers, SESSION_COOKIE)?;
    state.auth.authenticate(&cookie_token)
}

fn pairing_response(state: &ConsoleState, denied: bool) -> Response {
    let Some(PairingView {
        cookie_token,
        csrf_token,
    }) = state.auth.pairing()
    else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let status = if denied {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::OK
    };
    let mut response = views::pair(status, &csrf_token, denied);
    if append_cookie(
        &mut response,
        PAIRING_COOKIE,
        &cookie_token,
        state.config.limits().bootstrap_lifetime_seconds,
    ) {
        response
    } else {
        views::locked(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut found = None;
    for header in headers.get_all(COOKIE) {
        let value = header.to_str().ok()?;
        for part in value.split(';') {
            let (cookie_name, cookie_value) = part.trim().split_once('=')?;
            if cookie_name == name {
                if found.is_some() || !valid_token(cookie_value) {
                    return None;
                }
                found = Some(cookie_value.to_owned());
            }
        }
    }
    found
}

fn valid_token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn append_cookie(response: &mut Response, name: &str, value: &str, max_age: u64) -> bool {
    let cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}");
    let Ok(header) = HeaderValue::from_str(&cookie) else {
        return false;
    };
    response.headers_mut().append(SET_COOKIE, header);
    true
}

fn append_clear_cookie(response: &mut Response, name: &str) -> bool {
    let cookie = format!("{name}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    let Ok(header) = HeaderValue::from_str(&cookie) else {
        return false;
    };
    response.headers_mut().append(SET_COOKIE, header);
    true
}
