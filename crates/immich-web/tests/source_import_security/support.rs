use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use axum::Router;
use axum::http::header::LOCATION;
use axum::http::{Method, StatusCode};
use immich_rs_web::{WebConfig, WebConsole};

use super::Session;
use super::mock::SYNTHETIC_KEY;
use super::support::{HOST, ORIGIN, SECRET, TestWorkspace, csrf, response_cookie, send};

pub fn create_sources(workspace: &TestWorkspace) -> Result<(), Box<dyn std::error::Error>> {
    let apple = workspace.path("sources/apple/Synthetic Album");
    let picasa = workspace.path("sources/picasa/Synthetic Album");
    fs::create_dir_all(workspace.path("sources"))?;
    fs::create_dir_all(&apple)?;
    fs::create_dir_all(&picasa)?;
    let file = File::create(workspace.path("sources/takeout.zip"))?;
    let mut archive = zip::ZipWriter::new(file);
    archive.start_file(
        "Takeout/Google Photos/Photos from 2024/takeout.jpg",
        zip::write::SimpleFileOptions::default(),
    )?;
    archive.write_all(b"synthetic takeout\n")?;
    archive.finish()?;
    fs::write(apple.join("apple.jpg"), b"synthetic apple\n")?;
    fs::write(picasa.join("picasa.jpg"), b"synthetic picasa\n")?;
    fs::write(
        picasa.join(".picasa.ini"),
        b"[Picasa]\nname=Synthetic Album\n[picasa.jpg]\ncaption=Synthetic\n",
    )?;
    Ok(())
}

pub fn configured_console(
    workspace: &TestWorkspace,
    origin: &str,
) -> Result<WebConsole, Box<dyn std::error::Error>> {
    let bootstrap = workspace.path("bootstrap.secret");
    let api_key = workspace.path("api-key.secret");
    fs::write(&bootstrap, format!("{SECRET}\n"))?;
    fs::write(&api_key, format!("{SYNTHETIC_KEY}\n"))?;
    make_private(&bootstrap)?;
    make_private(&api_key)?;
    let config = format!(
        r#"schema_version = 1
[web]
listen_address = "127.0.0.1:2285"
public_origin = "http://127.0.0.1:2285"
bootstrap_secret_file = {}
history_state_id = "console"
{}
[[servers]]
id = "disposable"
origin = "{}"
api_key_file = {}
mode = "disposable"
generation = 1
credential_generation = 1
[[states]]
id = "console"
label = "Synthetic state"
allowed_root = {}
relative_root = "state"
generation = 1
"#,
        toml_path(&bootstrap),
        source_profiles(workspace),
        origin,
        toml_path(&api_key),
        toml_path(&workspace.path("")),
    );
    let path = workspace.path("imports.toml");
    fs::write(&path, config)?;
    Ok(WebConsole::from_config(WebConfig::load(&path)?)?)
}

fn source_profiles(workspace: &TestWorkspace) -> String {
    let allowed = workspace.path("sources");
    [
        ("takeout", "takeout.zip", "Google Takeout", "google_takeout"),
        ("apple", "apple", "Apple Photos", "apple_photos"),
        ("picasa", "picasa", "Picasa", "picasa"),
    ]
    .into_iter()
    .map(|(id, input, label, kind)| {
        let adapter_options = match kind {
            "apple_photos" => "album_mode = \"path\"\nalbum_path_joiner = \" - \"",
            "picasa" => {
                "album_mode = \"folder\"\nalbum_path_joiner = \" / \"\npicasa_albums = true\nfilename_date = true"
            }
            _ => "",
        };
        format!(
            r#"[[sources]]
id = "{id}"
label = "Synthetic {label}"
kind = "{kind}"
allowed_root = {}
relative_inputs = ["{input}"]
generation = 1
[sources.scan]
buffer_bytes = 4096
max_entries = 100
max_directory_entries = 50
max_path_bytes = 512
[sources.options]
max_archives = 4
max_archive_entry_bytes = 1048576
max_compression_ratio = 20
compression_ratio_grace_bytes = 1024
{adapter_options}
[sources.upload]
verification_buffer_bytes = 4096
concurrency = 1
max_attempts_per_operation = 2
max_retries_per_run = 2
retry_base_delay_ms = 1
retry_delay_cap_ms = 2
"#,
            toml_path(&allowed)
        )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn toml_path(path: &Path) -> String {
    serde_json::Value::String(path.to_string_lossy().into_owned()).to_string()
}

pub async fn pair(router: &Router) -> Result<Session, Box<dyn std::error::Error>> {
    let page = send(router, Method::GET, "/pair", Some(HOST), None, None, "").await?;
    let cookie = response_cookie(&page.headers, "immich_rs_pairing")?;
    let token = csrf(&page.body)?;
    let response = send(
        router,
        Method::POST,
        "/pair",
        Some(HOST),
        Some(ORIGIN),
        Some(&cookie),
        &format!("csrf={token}&secret={SECRET}"),
    )
    .await?;
    let cookie = response_cookie(&response.headers, "immich_rs_session")?;
    let page = send(
        router,
        Method::GET,
        "/",
        Some(HOST),
        None,
        Some(&cookie),
        "",
    )
    .await?;
    Ok(Session {
        cookie,
        csrf: csrf(&page.body)?.to_owned(),
    })
}

pub async fn admit(
    router: &Router,
    session: &Session,
    path: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = send(
        router,
        Method::POST,
        path,
        Some(HOST),
        Some(ORIGIN),
        Some(&session.cookie),
        &format!("csrf={}", session.csrf),
    )
    .await?;
    assert_eq!(response.status, StatusCode::SEE_OTHER);
    response
        .headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| "job location missing".into())
}

pub async fn wait_terminal(
    router: &Router,
    session: &Session,
    path: &str,
    context: &str,
    expect_failure: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    for _attempt in 0..300 {
        let page = send(
            router,
            Method::GET,
            path,
            Some(HOST),
            None,
            Some(&session.cookie),
            "",
        )
        .await?;
        let failed = page.body.contains("Failed ·");
        if page.body.contains("Completed ·") || failed {
            if failed != expect_failure {
                return Err(format!("unexpected source import result for {context}").into());
            }
            return Ok(page.body);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err("source import job did not finish".into())
}

pub fn link_reference(body: &str, marker: &str) -> Option<String> {
    let remaining = body.get(body.find(marker)?.saturating_add(marker.len())..)?;
    remaining
        .find('"')
        .and_then(|end| remaining.get(..end))
        .map(str::to_owned)
}

pub fn definition(body: &str, label: &str) -> Result<String, Box<dyn std::error::Error>> {
    let marker = format!("<dt>{label}</dt><dd>");
    between(body, &marker)
}

pub fn between(body: &str, marker: &str) -> Result<String, Box<dyn std::error::Error>> {
    let remaining = body
        .get(body.find(marker).ok_or("definition missing")? + marker.len()..)
        .ok_or("definition invalid")?;
    let end = remaining.find('<').ok_or("definition value invalid")?;
    Ok(remaining
        .get(..end)
        .ok_or("definition value missing")?
        .to_owned())
}

#[cfg(unix)]
fn make_private(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn make_private(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
