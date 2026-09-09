use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::grants::GrantView;
use crate::jobs::{JobSnapshot, JobStatus};
use crate::state_store::{DryRunReceipt, StoredHistory};
use crate::{ServerProfile, SourceProfile};

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
    servers: Vec<ServerView<'a>>,
}

struct ServerView<'a> {
    id: &'a str,
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
    pub terminal: &'static str,
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
    pub plan_ref: String,
    pub receipt_ref: String,
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
            terminal: if snapshot.status.terminal() {
                "true"
            } else {
                "false"
            },
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
            plan_ref: snapshot
                .artifact
                .as_ref()
                .map_or_else(String::new, |artifact| artifact.reference.encode()),
            receipt_ref: snapshot
                .receipt
                .map_or_else(String::new, crate::state_store::ReceiptRef::encode),
        }
    }
}

#[derive(Template)]
#[template(path = "scan_failed.html")]
struct ScanFailedTemplate;

#[derive(Template)]
#[template(path = "history.html")]
struct HistoryTemplate {
    rows: Vec<HistoryRow>,
}

struct HistoryRow {
    recorded_unix: i64,
    kind: &'static str,
    status: &'static str,
    assets: u64,
    sidecars: u64,
    bytes_read: u64,
    warnings: u64,
    errors: u64,
    has_plan: bool,
    plan_ref: String,
}

#[derive(Template)]
#[template(path = "plan.html")]
struct PlanTemplate<'a> {
    reference: String,
    schema_version: u32,
    plan_sha256: &'a str,
    source_profile_id: &'a str,
    server_profile_id: &'a str,
    operations: u64,
    media_bytes: u64,
    sidecars: u64,
    max_logical_effects: u64,
    csrf_token: &'a str,
}

#[derive(Template)]
#[template(path = "receipt.html")]
struct ReceiptTemplate<'a> {
    reference: String,
    plan_reference: String,
    plan_sha256: &'a str,
    source_configuration_sha256: &'a str,
    server_identity_sha256: &'a str,
    server_profile_sha256: &'a str,
    credential_generation: u64,
    max_logical_effects: u64,
    completed_unix: i64,
    csrf_token: &'a str,
    grant_ready: bool,
    idempotency_key: String,
    production: bool,
}

pub fn pair(status: StatusCode, csrf_token: &str, denied: bool) -> Response {
    render(status, &PairTemplate { csrf_token, denied })
}

pub fn dashboard(
    csrf_token: &str,
    profiles: &[SourceProfile],
    servers: &[ServerProfile],
) -> Response {
    let sources = profiles
        .iter()
        .map(|profile| SourceView {
            id: profile.id(),
            label: profile.label(),
            servers: servers
                .iter()
                .map(|server| ServerView { id: server.id() })
                .collect(),
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

pub fn history(records: &[StoredHistory]) -> Response {
    let rows = records
        .iter()
        .map(|stored| HistoryRow {
            recorded_unix: stored.record.recorded_unix,
            kind: stored.record.kind.label(),
            status: stored.record.status.label(),
            assets: stored.record.counters.assets,
            sidecars: stored.record.counters.sidecars,
            bytes_read: stored.record.counters.bytes_read,
            warnings: stored.record.counters.warnings,
            errors: stored.record.counters.errors,
            has_plan: stored.record.plan.is_some(),
            plan_ref: stored
                .record
                .plan
                .map_or_else(String::new, |(reference, _)| reference.encode()),
        })
        .collect();
    render(StatusCode::OK, &HistoryTemplate { rows })
}

pub fn plan(stored: &crate::state_store::StoredUploadPlan, csrf_token: &str) -> Response {
    render(
        StatusCode::OK,
        &PlanTemplate {
            reference: stored.reference.encode(),
            schema_version: stored.plan.schema_version,
            plan_sha256: &stored.binding.plan_sha256,
            source_profile_id: &stored.binding.source_profile_id,
            server_profile_id: &stored.binding.server_profile_id,
            operations: stored.plan.summary.operations,
            media_bytes: stored.plan.summary.media_bytes,
            sidecars: stored.plan.summary.xmp_sidecars,
            max_logical_effects: stored.binding.max_logical_effects,
            csrf_token,
        },
    )
}

pub fn receipt(receipt: &DryRunReceipt, csrf_token: &str, grant: Option<&GrantView>) -> Response {
    render(
        StatusCode::OK,
        &ReceiptTemplate {
            reference: receipt.reference.encode(),
            plan_reference: receipt.binding.plan_reference.encode(),
            plan_sha256: &receipt.binding.plan_sha256,
            source_configuration_sha256: &receipt.binding.source_configuration_sha256,
            server_identity_sha256: &receipt.binding.server_identity_sha256,
            server_profile_sha256: &receipt.binding.server_profile_sha256,
            credential_generation: receipt.binding.credential_generation,
            max_logical_effects: receipt.binding.max_logical_effects,
            completed_unix: receipt.binding.completed_unix,
            csrf_token,
            grant_ready: grant.is_some(),
            idempotency_key: grant.map_or_else(String::new, |value| value.idempotency_key.clone()),
            production: grant.is_some_and(|value| value.production),
        },
    )
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
