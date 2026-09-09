use axum::Router;
use axum::body::Body;
use axum::extract::{Form, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;

use crate::auth::SessionView;
use crate::cookies::{SESSION_COOKIE, value as cookie_value};
use crate::events_http;
use crate::grants::{GrantAdmission, GrantError, GrantRequest};
use crate::http::ConsoleState;
use crate::jobs::AdmissionError;
use crate::state_store::{ArtifactRef, ReceiptRef};
use crate::{views, views::JobView};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsrfForm {
    csrf: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmForm {
    csrf: String,
    plan_sha256: String,
    max_logical_effects: u64,
    backup_reference: String,
    acknowledge: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyForm {
    csrf: String,
    idempotency_key: String,
}

pub fn routes() -> Router<ConsoleState> {
    Router::new()
        .route("/sources/{source_id}/scan", post(admit_folder))
        .route(
            "/sources/{source_id}/servers/{server_id}/plan",
            post(admit_folder_plan),
        )
        .route("/jobs/{job_id}", get(job_status))
        .route("/jobs/{job_id}/events", get(events_http::job_events))
        .route("/jobs/{job_id}/cancel", post(cancel_job))
        .route("/plans/{plan_ref}", get(inspect_plan))
        .route("/plans/{plan_ref}/export", get(export_plan))
        .route("/plans/{plan_ref}/dry-run", post(admit_dry_run))
        .route("/receipts/{receipt_ref}", get(inspect_receipt))
        .route("/receipts/{receipt_ref}/confirm", post(confirm_apply))
        .route("/receipts/{receipt_ref}/apply", post(admit_apply))
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

async fn admit_folder_plan(
    State(state): State<ConsoleState>,
    Path((source_id, server_id)): Path<(String, String)>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    match state
        .jobs
        .admit_plan(session.binding, &source_id, &server_id)
    {
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

async fn inspect_plan(
    State(state): State<ConsoleState>,
    Path(plan_ref): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = authenticated(&state, &headers) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(reference) = ArtifactRef::parse(&plan_ref) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    if !state_is_current(&state) {
        return views::scan_failed(StatusCode::SERVICE_UNAVAILABLE);
    }
    state.store.plans().load(reference).map_or_else(
        |_| views::scan_failed(StatusCode::NOT_FOUND),
        |stored| views::plan(&stored, &session.csrf_token),
    )
}

async fn admit_dry_run(
    State(state): State<ConsoleState>,
    Path(plan_ref): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    let Some(reference) = ArtifactRef::parse(&plan_ref) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    if !state_is_current(&state) {
        return views::scan_failed(StatusCode::SERVICE_UNAVAILABLE);
    }
    match state.jobs.admit_dry_run(session.binding, reference) {
        Ok(job_id) => Redirect::to(&format!("/jobs/{job_id}")).into_response(),
        Err(AdmissionError::UnknownProfile) => views::scan_failed(StatusCode::NOT_FOUND),
        Err(AdmissionError::Full) => views::scan_failed(StatusCode::TOO_MANY_REQUESTS),
        Err(AdmissionError::Unavailable) => views::scan_failed(StatusCode::SERVICE_UNAVAILABLE),
    }
}

async fn inspect_receipt(
    State(state): State<ConsoleState>,
    Path(receipt_ref): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = authenticated(&state, &headers) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(reference) = ReceiptRef::parse(&receipt_ref) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    if !state_is_current(&state) {
        return views::scan_failed(StatusCode::SERVICE_UNAVAILABLE);
    }
    state
        .store
        .history()
        .dry_run_receipt(reference)
        .map_or_else(
            |_| views::scan_failed(StatusCode::SERVICE_UNAVAILABLE),
            |receipt| {
                receipt.map_or_else(
                    || views::scan_failed(StatusCode::NOT_FOUND),
                    |value| views::receipt(&value, &session.csrf_token, None),
                )
            },
        )
}

async fn confirm_apply(
    State(state): State<ConsoleState>,
    Path(receipt_ref): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ConfirmForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    let Some(reference) = ReceiptRef::parse(&receipt_ref) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    if form.acknowledge != "apply" || !state_is_current(&state) {
        return views::scan_failed(StatusCode::FORBIDDEN);
    }
    let request = GrantRequest {
        receipt: reference,
        plan_sha256: form.plan_sha256,
        max_logical_effects: form.max_logical_effects,
        backup_reference: form.backup_reference,
    };
    let grant = match state.grants.confirm(session.binding, request) {
        Ok(value) => value,
        Err(error) => return grant_error(error),
    };
    state
        .store
        .history()
        .dry_run_receipt(reference)
        .map_or_else(
            |_| views::scan_failed(StatusCode::SERVICE_UNAVAILABLE),
            |receipt| {
                receipt.map_or_else(
                    || views::scan_failed(StatusCode::NOT_FOUND),
                    |value| views::receipt(&value, &session.csrf_token, Some(&grant)),
                )
            },
        )
}

async fn admit_apply(
    State(state): State<ConsoleState>,
    Path(receipt_ref): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ApplyForm>,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(session) = state.auth.authenticate_csrf(&cookie_token, &form.csrf) else {
        return views::locked(StatusCode::FORBIDDEN);
    };
    let Some(reference) = ReceiptRef::parse(&receipt_ref) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    let admitted = state.grants.admit_with(
        session.binding,
        reference,
        &form.idempotency_key,
        |capability| state.jobs.admit_apply(capability),
    );
    match admitted {
        Ok(GrantAdmission::Admitted(job_id) | GrantAdmission::Existing(job_id)) => {
            Redirect::to(&format!("/jobs/{job_id}")).into_response()
        }
        Err(error) => grant_error(error),
    }
}

fn grant_error(error: GrantError) -> Response {
    match error {
        GrantError::Invalid => views::scan_failed(StatusCode::FORBIDDEN),
        GrantError::Expired => views::scan_failed(StatusCode::GONE),
        GrantError::Unavailable => views::scan_failed(StatusCode::SERVICE_UNAVAILABLE),
        GrantError::Admission(AdmissionError::UnknownProfile) => {
            views::scan_failed(StatusCode::NOT_FOUND)
        }
        GrantError::Admission(AdmissionError::Full) => {
            views::scan_failed(StatusCode::TOO_MANY_REQUESTS)
        }
        GrantError::Admission(AdmissionError::Unavailable) => {
            views::scan_failed(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

async fn export_plan(
    State(state): State<ConsoleState>,
    Path(plan_ref): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(_session) = authenticated(&state, &headers) else {
        return views::locked(StatusCode::UNAUTHORIZED);
    };
    let Some(reference) = ArtifactRef::parse(&plan_ref) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    if !state_is_current(&state) {
        return views::scan_failed(StatusCode::SERVICE_UNAVAILABLE);
    }
    let Ok((file, length)) = state.store.plans().open_export(reference) else {
        return views::scan_failed(StatusCode::NOT_FOUND);
    };
    let Ok(content_length) = HeaderValue::from_str(&length.to_string()) else {
        return views::scan_failed(StatusCode::SERVICE_UNAVAILABLE);
    };
    let stream = tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file));
    (
        StatusCode::OK,
        [
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (
                CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=immich-rs-plan.json"),
            ),
            (CONTENT_LENGTH, content_length),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

fn state_is_current(state: &ConsoleState) -> bool {
    state
        .config
        .history_state()
        .and_then(crate::StateProfile::resolve)
        .is_ok_and(|resolved| state.store.matches_state(&resolved))
}

fn authenticated(state: &ConsoleState, headers: &HeaderMap) -> Option<SessionView> {
    let cookie_token = cookie_value(headers, SESSION_COOKIE)?;
    state.auth.authenticate(&cookie_token)
}
