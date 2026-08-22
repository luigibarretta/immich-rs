use immich_rs_client::{ApiKey, ClientConfig, ImmichEndpoint, ImmichReadClient};

use crate::failure::CliFailure;

const API_KEY_ENVIRONMENT: &str = "IMMICH_RS_API_KEY";
const API_KEY_FILE_ENVIRONMENT: &str = "IMMICH_RS_API_KEY_FILE";
const MAX_API_KEY_FILE_BYTES: u64 = 4_097;

pub fn read_client(server: &str) -> Result<ImmichReadClient, CliFailure> {
    read_client_with_config(server, ClientConfig::default())
}

pub fn archive_client(server: &str) -> Result<ImmichReadClient, CliFailure> {
    let config = ClientConfig {
        max_response_bytes: 1_024 * 1_024,
        ..ClientConfig::default()
    };
    read_client_with_config(server, config)
}

fn read_client_with_config(
    server: &str,
    config: ClientConfig,
) -> Result<ImmichReadClient, CliFailure> {
    let endpoint = ImmichEndpoint::parse(server)
        .map_err(|_| CliFailure::usage("invalid or non-loopback Phase-2 server"))?;
    if !endpoint.is_loopback() {
        return Err(CliFailure::usage(
            "Phase-2 server must resolve to a literal loopback origin",
        ));
    }
    let key_value = api_key_value()?;
    let api_key = ApiKey::new(&key_value).map_err(|_| CliFailure::authentication())?;
    ImmichReadClient::new(endpoint, api_key, config).map_err(CliFailure::from_client)
}

fn api_key_value() -> Result<String, CliFailure> {
    let direct = std::env::var_os(API_KEY_ENVIRONMENT);
    let file = std::env::var_os(API_KEY_FILE_ENVIRONMENT);
    match (direct, file) {
        (Some(_), Some(_)) | (None, None) => Err(CliFailure::authentication()),
        (Some(value), None) => value
            .into_string()
            .map_err(|_| CliFailure::authentication()),
        (None, Some(path)) => read_api_key_file(&std::path::PathBuf::from(path)),
    }
}

fn read_api_key_file(path: &std::path::Path) -> Result<String, CliFailure> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| CliFailure::authentication())?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_API_KEY_FILE_BYTES
    {
        return Err(CliFailure::authentication());
    }
    let value = std::fs::read_to_string(path).map_err(|_| CliFailure::authentication())?;
    let value = value.strip_suffix('\n').unwrap_or(&value);
    let value = value.strip_suffix('\r').unwrap_or(value);
    Ok(value.to_owned())
}
