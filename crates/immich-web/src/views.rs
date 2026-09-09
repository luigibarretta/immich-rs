use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::SourceProfile;

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
