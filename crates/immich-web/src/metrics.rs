use axum::Router;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::http::{ConsoleState, authenticated_session};
use crate::views;

const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

pub fn routes() -> Router<ConsoleState> {
    Router::new().route("/metrics", get(metrics))
}

async fn metrics(
    State(state): State<ConsoleState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let browser_authorized = authenticated_session(&state, &headers).is_some();
    let machine_authorized = state
        .metrics_auth
        .as_ref()
        .is_some_and(|auth| auth.authorize(peer.ip(), &headers));
    if !browser_authorized && !machine_authorized {
        return views::locked(StatusCode::UNAUTHORIZED);
    }
    let Some(sessions) = state.auth.active_session_count() else {
        return views::locked(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(jobs) = state.jobs.aggregate_metrics() else {
        return views::locked(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Ok(history_rows) = state.store.history().count() else {
        return views::locked(StatusCode::SERVICE_UNAVAILABLE);
    };
    let body = format!(
        concat!(
            "# HELP immich_rs_web_sessions_active Active authenticated sessions.\n",
            "# TYPE immich_rs_web_sessions_active gauge\n",
            "immich_rs_web_sessions_active {}\n",
            "# HELP immich_rs_web_jobs_queued Queued operator jobs.\n",
            "# TYPE immich_rs_web_jobs_queued gauge\n",
            "immich_rs_web_jobs_queued {}\n",
            "# HELP immich_rs_web_jobs_running Running operator jobs.\n",
            "# TYPE immich_rs_web_jobs_running gauge\n",
            "immich_rs_web_jobs_running {}\n",
            "# HELP immich_rs_web_jobs_completed Retained completed jobs.\n",
            "# TYPE immich_rs_web_jobs_completed gauge\n",
            "immich_rs_web_jobs_completed {}\n",
            "# HELP immich_rs_web_jobs_failed Retained failed jobs.\n",
            "# TYPE immich_rs_web_jobs_failed gauge\n",
            "immich_rs_web_jobs_failed {}\n",
            "# HELP immich_rs_web_jobs_cancelled Retained cancelled jobs.\n",
            "# TYPE immich_rs_web_jobs_cancelled gauge\n",
            "immich_rs_web_jobs_cancelled {}\n",
            "# HELP immich_rs_web_jobs_retained Jobs retained in memory.\n",
            "# TYPE immich_rs_web_jobs_retained gauge\n",
            "immich_rs_web_jobs_retained {}\n",
            "# HELP immich_rs_web_sse_subscribers Active event-stream subscribers.\n",
            "# TYPE immich_rs_web_sse_subscribers gauge\n",
            "immich_rs_web_sse_subscribers {}\n",
            "# HELP immich_rs_web_history_rows Retained terminal history rows.\n",
            "# TYPE immich_rs_web_history_rows gauge\n",
            "immich_rs_web_history_rows {}\n"
        ),
        sessions,
        jobs.queued,
        jobs.running,
        jobs.completed,
        jobs.failed,
        jobs.cancelled,
        jobs.retained,
        jobs.subscribers,
        history_rows,
    );
    ([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], body).into_response()
}
