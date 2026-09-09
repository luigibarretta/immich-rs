#![forbid(unsafe_code)]

use axum::Router;
use axum::http::header::{CACHE_CONTROL, LOCATION};
use axum::http::{Method, StatusCode};

mod support;

use support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

struct Session {
    cookie: String,
    csrf: String,
}

async fn pair(router: &Router) -> Result<Session, Box<dyn std::error::Error>> {
    let page = send(router, Method::GET, "/pair", Some(HOST), None, None, "").await?;
    assert_eq!(page.status, StatusCode::OK);
    assert_accessible_shell(&page.body);
    let pairing_cookie = response_cookie(&page.headers, "immich_rs_pairing")?;
    let pairing_csrf = csrf(&page.body)?.to_owned();
    let response = send(
        router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&pairing_cookie),
        &format!("csrf={pairing_csrf}&secret={SECRET}"),
    )
    .await?;
    if response.status != StatusCode::SEE_OTHER {
        return Err("pairing did not redirect".into());
    }
    let cookie = response_cookie(&response.headers, "immich_rs_session")?;
    let dashboard = send(
        router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&cookie),
        "",
    )
    .await?;
    assert_accessible_shell(&dashboard.body);
    assert_safe_html(&dashboard.body);
    Ok(Session {
        cookie,
        csrf: csrf(&dashboard.body)?.to_owned(),
    })
}

fn assert_accessible_shell(body: &str) {
    for required in [
        "<html lang=\"en\">",
        "<meta name=\"viewport\"",
        "class=\"skip-link\" href=\"#main-content\"",
        "<nav aria-label=\"Primary\">",
        "<main id=\"main-content\" tabindex=\"-1\">",
        "<h1",
    ] {
        assert!(body.contains(required), "missing shell marker: {required}");
    }
}

fn assert_safe_html(body: &str) {
    for forbidden in [
        " onclick=",
        " onload=",
        " onerror=",
        "javascript:",
        "href=\"http://",
        "href=\"https://",
        "src=\"http://",
        "src=\"https://",
        "<style",
    ] {
        assert!(!body.contains(forbidden), "unsafe HTML marker: {forbidden}");
    }
}

async fn admit_scan(
    router: &Router,
    session: &Session,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = send(
        router,
        Method::POST,
        "/sources/camera_roll/scan",
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    if response.status != StatusCode::SEE_OTHER {
        return Err("scan admission did not redirect".into());
    }
    Ok(response
        .headers
        .get(LOCATION)
        .ok_or("scan redirect missing")?
        .to_str()?
        .to_owned())
}

async fn wait_for_completion(
    router: &Router,
    session: &Session,
    location: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    for _attempt in 0..100 {
        let response = send(
            router,
            Method::GET,
            location,
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        if response.body.contains("Completed ·") {
            return Ok(response.body);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Err("scan did not finish within the bounded wait".into())
}

#[tokio::test]
async fn server_rendered_flow_keeps_accessible_status_and_local_assets()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace = TestWorkspace::new()?;
    let router = workspace.console()?.router();
    let session = pair(&router).await?;
    let location = admit_scan(&router, &session).await?;
    let job = wait_for_completion(&router, &session, &location).await?;
    assert_accessible_shell(&job);
    assert_safe_html(&job);
    assert!(job.contains("role=\"status\" aria-live=\"polite\" aria-atomic=\"true\""));
    assert!(job.contains("id=\"polling-fallback\""));
    assert_eq!(job.matches("<script").count(), 1);
    assert!(job.contains("<script src=\"/assets/job.js\" defer>"));

    let history = send(
        &router,
        Method::GET,
        "/history",
        Some(HOST),
        None,
        Some(&session.cookie),
        "",
    )
    .await?;
    assert_eq!(history.status, StatusCode::OK);
    assert_accessible_shell(&history.body);
    assert_safe_html(&history.body);
    assert!(
        history
            .body
            .contains("role=\"region\" aria-label=\"Terminal history table\"")
    );
    assert_eq!(history.body.matches("<th scope=\"col\">").count(), 9);

    let css = send(
        &router,
        Method::GET,
        "/assets/console.css",
        Some(HOST),
        None,
        None,
        "",
    )
    .await?;
    assert_eq!(css.status, StatusCode::OK);
    assert!(css.body.contains(".skip-link:focus"));
    assert!(css.body.contains(":focus-visible"));
    assert!(css.body.contains("prefers-reduced-motion: reduce"));
    assert!(!css.body.contains("@import"));

    let script = send(
        &router,
        Method::GET,
        "/assets/job.js",
        Some(HOST),
        None,
        None,
        "",
    )
    .await?;
    assert_eq!(script.status, StatusCode::OK);
    assert_eq!(
        script
            .headers
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store, max-age=0")
    );
    assert!(script.body.contains(".textContent ="));
    for forbidden in [
        "innerHTML",
        "outerHTML",
        "document.write",
        "insertAdjacentHTML",
        "eval(",
    ] {
        assert!(!script.body.contains(forbidden));
    }
    assert!(!job.contains(&workspace.path("").display().to_string()));
    Ok(())
}
