use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, Form, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use crate::auth::{AuthStore, PairingFailure, PairingView, SessionView};
use crate::cookies::{
    PAIRING_COOKIE, SESSION_COOKIE, append as append_cookie, clear as append_clear_cookie,
    value as cookie_value,
};
use crate::grants::GrantStore;
use crate::job_http;
use crate::jobs::JobManager;
use crate::metrics;
use crate::metrics_auth::MetricsAuthenticator;
use crate::oidc::OidcManager;
use crate::oidc_http;
use crate::policy::{RequestPolicy, request_policy};
use crate::server;
use crate::state_store::ConsoleStore;
use crate::tls;
use crate::{WebConfig, WebConfigError, views};

const STYLESHEET: &str = include_str!("../assets/console.css");
const JOB_SCRIPT: &str = include_str!("../assets/job.js");

/// Authenticated operator console assembled from validated loopback or LAN configuration.
pub struct WebConsole {
    state: ConsoleState,
    tls: Option<TlsAcceptor>,
}

#[derive(Clone)]
pub struct ConsoleState {
    pub config: Arc<WebConfig>,
    pub auth: Arc<AuthStore>,
    pub jobs: JobManager,
    pub grants: Arc<GrantStore>,
    pub store: Arc<ConsoleStore>,
    pub(crate) metrics_auth: Option<Arc<MetricsAuthenticator>>,
    pub(crate) oidc: Option<Arc<OidcManager>>,
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
    /// Load the configured authentication boundary and initialize ephemeral state.
    pub fn from_config(config: WebConfig) -> Result<Self, WebConfigError> {
        let auth = match config.bootstrap_secret_file() {
            Some(bootstrap) => AuthStore::load(bootstrap, config.limits())?,
            None => AuthStore::oidc(config.limits()),
        };
        let tls = config.lan().map(tls::load_acceptor).transpose()?;
        let metrics_auth = config
            .metrics()
            .map(MetricsAuthenticator::load)
            .transpose()?
            .map(Arc::new);
        let state_profile = config.history_state()?.resolve()?;
        let store = Arc::new(ConsoleStore::open(&state_profile, config.limits())?);
        let config = Arc::new(config);
        let oidc = config
            .lan()
            .map(|_| OidcManager::new(Arc::clone(&config)).map(Arc::new))
            .transpose()?;
        let jobs = JobManager::new(Arc::clone(&config), Arc::clone(&store));
        let grants = Arc::new(GrantStore::new(Arc::clone(&config), Arc::clone(&store)));
        Ok(Self {
            state: ConsoleState {
                config,
                auth: Arc::new(auth),
                jobs,
                grants,
                store,
                metrics_auth,
                oidc,
            },
            tls,
        })
    }

    /// Build the bounded authenticated operator router.
    pub fn router(&self) -> Router {
        let limits = self.state.config.limits();
        let policy = RequestPolicy::new(&self.state.config);
        Router::new()
            .route("/", get(index))
            .route("/pair", get(pair_page).post(pair))
            .route("/logout", post(logout))
            .route("/assets/console.css", get(stylesheet))
            .route("/assets/job.js", get(job_script))
            .merge(oidc_http::routes())
            .merge(job_http::routes())
            .merge(metrics::routes())
            .fallback(not_found)
            .layer(RequestBodyLimitLayer::new(limits.request_body_bytes))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(limits.response_seconds),
            ))
            .layer(middleware::from_fn_with_state(policy, request_policy))
            .with_state(self.state.clone())
    }

    /// Bind the configured address and serve with the required transport until shutdown.
    pub async fn serve<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future<Output = ()>,
    {
        let listener = TcpListener::bind(self.state.config.listen_address()).await?;
        self.serve_on(listener, shutdown).await
    }

    /// Serve a pre-bound configured listener for hardened development and tests.
    pub async fn serve_on<Shutdown>(
        self,
        listener: TcpListener,
        shutdown: Shutdown,
    ) -> io::Result<()>
    where
        Shutdown: Future<Output = ()>,
    {
        if listener.local_addr()? != self.state.config.listen_address() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "listener does not match configured web address",
            ));
        }
        let limits = self.state.config.limits();
        let router = self.router();
        let result = server::serve(listener, router, limits, self.tls, shutdown).await;
        let jobs_clean = self.state.jobs.shutdown();
        if !jobs_clean && result.is_ok() {
            return Err(io::Error::other("web job worker failed"));
        }
        result
    }
}

async fn index(State(state): State<ConsoleState>, headers: HeaderMap) -> Response {
    if let Some(session) = authenticated_session(&state, &headers) {
        return views::dashboard(
            &session.csrf_token,
            state.config.sources(),
            state.config.servers(),
        );
    }
    if state.oidc.is_some() {
        oidc_http::login_response(&state, false)
    } else {
        pairing_response(&state, false)
    }
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
                state.config.secure_cookies(),
            ) && append_clear_cookie(
                &mut response,
                PAIRING_COOKIE,
                state.config.secure_cookies(),
            ) {
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
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    if !state.auth.logout(&cookie_token, &form.csrf) {
        return views::locked(StatusCode::FORBIDDEN);
    }
    state.grants.revoke_owner(&session.binding);
    let mut response = Redirect::to("/").into_response();
    if append_clear_cookie(&mut response, SESSION_COOKIE, state.config.secure_cookies()) {
        response
    } else {
        views::locked(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

async fn stylesheet() -> Response {
    ([(CONTENT_TYPE, "text/css; charset=utf-8")], STYLESHEET).into_response()
}

async fn job_script() -> Response {
    (
        [(CONTENT_TYPE, "application/javascript; charset=utf-8")],
        JOB_SCRIPT,
    )
        .into_response()
}

async fn not_found() -> Response {
    views::locked(StatusCode::NOT_FOUND)
}

pub fn authenticated_session(state: &ConsoleState, headers: &HeaderMap) -> Option<SessionView> {
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
        state.config.secure_cookies(),
    ) {
        response
    } else {
        views::locked(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
