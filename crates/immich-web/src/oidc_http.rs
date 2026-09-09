use std::net::SocketAddr;

use axum::Router;
use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use serde::Deserialize;

use crate::cookies::{
    OIDC_LOGIN_COOKIE, SESSION_COOKIE, append as append_cookie, clear as clear_cookie,
    value as cookie_value,
};
use crate::http::ConsoleState;
use crate::oidc::{LoginView, OidcIdentity};
use crate::views;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginForm {
    csrf: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallbackQuery {
    code: String,
    state: String,
}

pub fn routes() -> Router<ConsoleState> {
    Router::new()
        .route("/oidc/login", get(login_page).post(login))
        .route("/oidc/callback", get(callback))
}

pub fn login_response(state: &ConsoleState, denied: bool) -> Response {
    let Some(manager) = &state.oidc else {
        return views::locked(StatusCode::NOT_FOUND);
    };
    let LoginView {
        cookie_token,
        csrf_token,
    } = manager.login_view();
    let status = if denied {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::OK
    };
    let mut response = views::oidc_login(status, &csrf_token, denied);
    if append_cookie(
        &mut response,
        OIDC_LOGIN_COOKIE,
        &cookie_token,
        state.config.limits().oidc_state_seconds,
        true,
    ) {
        response
    } else {
        views::locked(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

async fn login_page(State(state): State<ConsoleState>) -> Response {
    login_response(&state, false)
}

async fn login(
    State(state): State<ConsoleState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let Some(manager) = &state.oidc else {
        return views::locked(StatusCode::NOT_FOUND);
    };
    let Some(login_cookie) = cookie_value(&headers, OIDC_LOGIN_COOKIE) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    let previous = cookie_value(&headers, SESSION_COOKIE);
    manager
        .begin(peer.ip(), &login_cookie, &form.csrf, previous)
        .await
        .map_or_else(
            |_| login_response(&state, true),
            |location| Redirect::to(&location).into_response(),
        )
}

async fn callback(
    State(state): State<ConsoleState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(manager) = &state.oidc else {
        return views::locked(StatusCode::NOT_FOUND);
    };
    let identity = manager.finish(peer.ip(), &query.state, &query.code).await;
    identity.map_or_else(
        |_| login_response(&state, true),
        |identity| establish_session(&state, &headers, &identity),
    )
}

fn establish_session(
    state: &ConsoleState,
    headers: &HeaderMap,
    identity: &OidcIdentity,
) -> Response {
    let current_cookie = cookie_value(headers, SESSION_COOKIE);
    let previous = identity
        .previous_cookie
        .as_deref()
        .or(current_cookie.as_deref());
    let previous_binding = previous.and_then(|cookie| state.auth.authenticate(cookie));
    let Ok(session) = state.auth.establish_oidc(&identity.principal, previous) else {
        return views::locked(StatusCode::SERVICE_UNAVAILABLE);
    };
    if let Some(previous) = previous_binding {
        state.grants.revoke_owner(&previous.binding);
    }
    let mut response = Redirect::to("/").into_response();
    if append_cookie(
        &mut response,
        SESSION_COOKIE,
        &session.cookie_token,
        state.config.limits().session_absolute_seconds,
        true,
    ) && clear_cookie(&mut response, OIDC_LOGIN_COOKIE, true)
    {
        response
    } else {
        views::locked(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
