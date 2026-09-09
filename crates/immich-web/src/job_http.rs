use axum::Router;
use axum::extract::{Form, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;

use crate::auth::SessionView;
use crate::cookies::{SESSION_COOKIE, value as cookie_value};
use crate::events_http;
use crate::http::ConsoleState;
use crate::jobs::AdmissionError;
use crate::{views, views::JobView};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsrfForm {
    csrf: String,
}

pub fn routes() -> Router<ConsoleState> {
    Router::new()
        .route("/sources/{source_id}/scan", post(admit_folder))
        .route("/jobs/{job_id}", get(job_status))
        .route("/jobs/{job_id}/events", get(events_http::job_events))
        .route("/jobs/{job_id}/cancel", post(cancel_job))
        .route("/history", get(history))
}

async fn history(State(state): State<ConsoleState>, headers: HeaderMap) -> Response {
    let Some(_session) = authenticated(&state, &headers) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Ok(records) = state
        .store
        .history()
        .latest(state.config.limits().history_page_rows)
    else {
        return views::scan_failed(StatusCode::SERVICE_UNAVAILABLE);
    };
    views::history(&records)
}

async fn admit_folder(
    State(state): State<ConsoleState>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    match state.jobs.admit(session.binding, &source_id) {
        Ok(job_id) => Redirect::to(&format!("/jobs/{job_id}")).into_response(),
        Err(AdmissionError::UnknownProfile) => views::scan_failed(StatusCode::NOT_FOUND),
        Err(AdmissionError::Full) => views::scan_failed(StatusCode::TOO_MANY_REQUESTS),
        Err(AdmissionError::Unavailable) => views::scan_failed(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn job_status(
    State(state): State<ConsoleState>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = authenticated(&state, &headers) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(snapshot) = state.jobs.snapshot(&job_id, &session.binding) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    views::job(&JobView::new(&snapshot, &session.csrf_token))
}

async fn cancel_job(
    State(state): State<ConsoleState>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    if state.jobs.cancel(&job_id, &session.binding).is_none() {
        return views::scan_failed(StatusCode::NOT_FOUND);
    }
    Redirect::to(&format!("/jobs/{job_id}")).into_response()
}

fn authenticated(state: &ConsoleState, headers: &HeaderMap) -> Option<SessionView> {
    let cookie_token = cookie_value(headers, SESSION_COOKIE)?;
    state.auth.authenticate(&cookie_token)
}
