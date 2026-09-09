use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::unfold;

use crate::auth::AuthStore;
use crate::cookies::{SESSION_COOKIE, value as cookie_value};
use crate::http::ConsoleState;
use crate::jobs::{JobEvent, JobSubscription, SubscribeError, SubscriptionDelivery};

const LAST_EVENT_ID: HeaderName = HeaderName::from_static("last-event-id");

pub async fn job_events(
    State(state): State<ConsoleState>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(cookie_token) = cookie_value(&headers, SESSION_COOKIE) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Some(session) = state.auth.authenticate(&cookie_token) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let after = match event_cursor(&headers) {
        Ok(after) => after,
        Err(status) => return status.into_response(),
    };
    let subscription = match state.jobs.subscribe(&job_id, session.binding, after) {
        Ok(subscription) => subscription,
        Err(error) => return subscription_status(error).into_response(),
    };
    let stream_state = EventStream {
        subscription,
        auth: Arc::clone(&state.auth),
        cookie_token,
        binding: session.binding,
        heartbeat: Duration::from_secs(state.config.limits().sse_heartbeat_seconds),
    };
    let stream = unfold(stream_state, |mut stream_state| async move {
        if !stream_state
            .auth
            .valid_binding(&stream_state.cookie_token, &stream_state.binding)
        {
            return None;
        }
        let delivery = stream_state
            .subscription
            .next(stream_state.heartbeat)
            .await?;
        if !stream_state
            .auth
            .valid_binding(&stream_state.cookie_token, &stream_state.binding)
        {
            return None;
        }
        let event = match delivery {
            SubscriptionDelivery::Event(event) => encode_job_event(event),
            SubscriptionDelivery::Heartbeat => Event::default().comment("heartbeat"),
            SubscriptionDelivery::ReplayExhausted => {
                Event::default().event("replay-exhausted").data("poll")
            }
        };
        Some((Ok::<Event, Infallible>(event), stream_state))
    });
    Sse::new(stream).into_response()
}

struct EventStream {
    subscription: JobSubscription,
    auth: Arc<AuthStore>,
    cookie_token: String,
    binding: [u8; 32],
    heartbeat: Duration,
}

fn event_cursor(headers: &HeaderMap) -> Result<Option<u64>, StatusCode> {
    let mut values = headers.get_all(&LAST_EVENT_ID).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let value = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

const fn subscription_status(error: SubscribeError) -> StatusCode {
    match error {
        SubscribeError::NotFound => StatusCode::NOT_FOUND,
        SubscribeError::InvalidCursor => StatusCode::BAD_REQUEST,
        SubscribeError::ReplayExhausted => StatusCode::CONFLICT,
        SubscribeError::SessionCapacity | SubscribeError::ProcessCapacity => {
            StatusCode::TOO_MANY_REQUESTS
        }
        SubscribeError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn encode_job_event(event: JobEvent) -> Event {
    let summary = event.summary.unwrap_or(crate::jobs::JobSummary {
        schema_version: 0,
        assets: 0,
        sidecars: 0,
        bytes_read: 0,
        warnings: 0,
        errors: 0,
    });
    let data = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        event.status.label(),
        stage_name(event),
        event.cancellation_requested,
        event.progress.assets_observed,
        event.progress.bytes_read,
        event.summary.is_some(),
        summary.schema_version,
        summary.assets,
        summary.sidecars,
        summary.bytes_read,
        summary.warnings,
        summary.errors,
    );
    Event::default()
        .event("job")
        .id(event.sequence.to_string())
        .data(data)
}

const fn stage_name(event: JobEvent) -> &'static str {
    use immich_rs_application::ProgressStage;

    match event.progress.stage {
        Some(ProgressStage::Discovery) => "discovery",
        Some(ProgressStage::ContentIdentity) => "identity",
        Some(ProgressStage::Reconciliation) => "reconciliation",
        Some(ProgressStage::Complete) => "complete",
        None => "starting",
    }
}
