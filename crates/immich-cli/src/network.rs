use immich_rs_client::{ApiKey, ClientConfig, ImmichEndpoint, ImmichReadClient};

use crate::failure::CliFailure;

const API_KEY_ENVIRONMENT: &str = "IMMICH_RS_API_KEY";

pub fn read_client(server: &str) -> Result<ImmichReadClient, CliFailure> {
    let endpoint = ImmichEndpoint::parse(server)
        .map_err(|_| CliFailure::usage("invalid or non-loopback Phase-2 server"))?;
    if !endpoint.is_loopback() {
        return Err(CliFailure::usage(
            "Phase-2 server must resolve to a literal loopback origin",
        ));
    }
    let key_value = std::env::var(API_KEY_ENVIRONMENT).map_err(|_| CliFailure::authentication())?;
    let api_key = ApiKey::new(&key_value).map_err(|_| CliFailure::authentication())?;
    ImmichReadClient::new(endpoint, api_key, ClientConfig::default())
        .map_err(CliFailure::from_client)
}
