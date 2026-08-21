use immich_rs_client::{ApiKey, ClientConfig, ImmichEndpoint, ImmichReadClient};

use crate::failure::CliFailure;

const API_KEY_ENVIRONMENT: &str = "IMMICH_RS_API_KEY";

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
    let key_value = std::env::var(API_KEY_ENVIRONMENT).map_err(|_| CliFailure::authentication())?;
    let api_key = ApiKey::new(&key_value).map_err(|_| CliFailure::authentication())?;
    ImmichReadClient::new(endpoint, api_key, config).map_err(CliFailure::from_client)
}
