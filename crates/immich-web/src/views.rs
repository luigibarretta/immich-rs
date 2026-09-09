use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::SourceProfile;
use crate::jobs::{JobSnapshot, JobStatus};

#[derive(Template)]
#[template(path = "pair.html")]
struct PairTemplate<'a> {
    csrf_token: &'a str,
    denied: bool,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate<'a> {
    csrf_token: &'a str,
    sources: Vec<SourceView<'a>>,
}

struct SourceView<'a> {
    id: &'a str,
    label: &'a str,
}

#[derive(Template)]
#[template(path = "locked.html")]
struct LockedTemplate;

#[derive(Template)]
#[template(path = "job.html")]
pub struct JobView<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub status: &'static str,
    pub stage: &'static str,
    pub cancellation_requested: bool,
    pub cancellable: bool,
    pub csrf_token: &'a str,
    pub assets_observed: u64,
    pub progress_bytes: u64,
    pub has_summary: bool,
    pub schema_version: u32,
    pub assets: u64,
    pub sidecars: u64,
    pub bytes_read: u64,
    pub warnings: usize,
    pub errors: usize,
}

impl<'a> JobView<'a> {
    pub fn new(snapshot: &'a JobSnapshot, csrf_token: &'a str) -> Self {
        let summary = snapshot.summary.unwrap_or(crate::jobs::JobSummary {
            schema_version: 0,
            assets: 0,
            sidecars: 0,
            bytes_read: 0,
            warnings: 0,
            errors: 0,
        });
        Self {
            id: &snapshot.id,
            label: &snapshot.source_label,
            status: snapshot.status.label(),
            stage: stage_label(snapshot),
            cancellation_requested: snapshot.cancellation_requested,
            cancellable: !snapshot.status.terminal() && !snapshot.cancellation_requested,
            csrf_token,
            assets_observed: snapshot.progress.assets_observed,
            progress_bytes: snapshot.progress.bytes_read,
            has_summary: snapshot.summary.is_some(),
            schema_version: summary.schema_version,
            assets: summary.assets,
            sidecars: summary.sidecars,
            bytes_read: summary.bytes_read,
            warnings: summary.warnings,
            errors: summary.errors,
        }
    }
}

#[derive(Template)]
#[template(path = "scan_failed.html")]
struct ScanFailedTemplate;

pub fn pair(status: StatusCode, csrf_token: &str, denied: bool) -> Response {
    render(status, &PairTemplate { csrf_token, denied })
}

pub fn dashboard(csrf_token: &str, profiles: &[SourceProfile]) -> Response {
    let sources = profiles
        .iter()
        .map(|profile| SourceView {
            id: profile.id(),
            label: profile.label(),
        })
        .collect();
    render(
        StatusCode::OK,
        &DashboardTemplate {
            csrf_token,
            sources,
        },
    )
}

pub fn locked(status: StatusCode) -> Response {
    render(status, &LockedTemplate)
}

pub fn job(view: &JobView<'_>) -> Response {
    render(StatusCode::OK, view)
}

fn stage_label(snapshot: &JobSnapshot) -> &'static str {
    use immich_rs_application::ProgressStage;

    match snapshot.progress.stage {
        Some(ProgressStage::Discovery) => "Discovering entries",
        Some(ProgressStage::ContentIdentity) => "Calculating content identity",
        Some(ProgressStage::Reconciliation) => "Reconciling metadata",
        Some(ProgressStage::Complete) => "Finalizing plan",
        None if snapshot.status == JobStatus::Queued => "Waiting for worker",
        None => "Starting",
    }
}

pub fn scan_failed(status: StatusCode) -> Response {
    render(status, &ScanFailedTemplate)
}

fn render<T: Template>(status: StatusCode, template: &T) -> Response {
    template.render().map_or_else(
        |_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "console rendering failed",
            )
                .into_response()
        },
        |body| (status, Html(body)).into_response(),
    )
}
