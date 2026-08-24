use immich_rs_client::{
    ApiKey, ClientConfig, EndpointAccess, ImmichEndpoint, ImmichReadClient, TlsRootCertificates,
};

use crate::failure::CliFailure;

const API_KEY_ENVIRONMENT: &str = "IMMICH_RS_API_KEY";
const API_KEY_FILE_ENVIRONMENT: &str = "IMMICH_RS_API_KEY_FILE";
const SOURCE_API_KEY_ENVIRONMENT: &str = "IMMICH_RS_SOURCE_API_KEY";
const SOURCE_API_KEY_FILE_ENVIRONMENT: &str = "IMMICH_RS_SOURCE_API_KEY_FILE";
const DESTINATION_API_KEY_ENVIRONMENT: &str = "IMMICH_RS_DESTINATION_API_KEY";
const DESTINATION_API_KEY_FILE_ENVIRONMENT: &str = "IMMICH_RS_DESTINATION_API_KEY_FILE";
const MAX_API_KEY_FILE_BYTES: u64 = 4_097;
const MAX_CA_CERTIFICATE_BYTES: u64 = 1024 * 1024;

pub fn read_client(
    server: &str,
    production_read: bool,
    ca_certificate: Option<&std::path::Path>,
) -> Result<ImmichReadClient, CliFailure> {
    read_client_with_config(
        server,
        ClientConfig::default(),
        production_read,
        ca_certificate,
    )
}

pub fn archive_client(
    server: &str,
    production_read: bool,
    ca_certificate: Option<&std::path::Path>,
) -> Result<ImmichReadClient, CliFailure> {
    let config = ClientConfig {
        max_response_bytes: 1_024 * 1_024,
        ..ClientConfig::default()
    };
    read_client_with_config(server, config, production_read, ca_certificate)
}

fn read_client_with_config(
    server: &str,
    config: ClientConfig,
    production_read: bool,
    ca_certificate: Option<&std::path::Path>,
) -> Result<ImmichReadClient, CliFailure> {
    let endpoint =
        ImmichEndpoint::parse(server).map_err(|_| CliFailure::usage("invalid server origin"))?;
    if production_read {
        EndpointAccess::production_read(&endpoint, true)
    } else {
        EndpointAccess::disposable(&endpoint)
    }
    .map_err(|_| CliFailure::usage("server does not match the authorized access mode"))?;
    if !production_read && ca_certificate.is_some() {
        return Err(CliFailure::usage(
            "custom CA certificates require production HTTPS mode",
        ));
    }
    let roots = ca_certificate.map(read_ca_certificate).transpose()?;
    let key_value = api_key_value(API_KEY_ENVIRONMENT, API_KEY_FILE_ENVIRONMENT)?;
    let api_key = ApiKey::new(&key_value).map_err(|_| CliFailure::authentication())?;
    let client = if let Some(roots) = roots {
        ImmichReadClient::new_production_read_with_roots(endpoint, api_key, config, true, roots)
    } else if production_read {
        ImmichReadClient::new_production_read(endpoint, api_key, config, true)
    } else {
        ImmichReadClient::new(endpoint, api_key, config)
    };
    client.map_err(CliFailure::from_client)
}

fn read_ca_certificate(path: &std::path::Path) -> Result<TlsRootCertificates, CliFailure> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| CliFailure::usage("cannot read CA certificate"))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_CA_CERTIFICATE_BYTES
    {
        return Err(CliFailure::usage(
            "CA certificate must be a bounded regular file",
        ));
    }
    let pem = std::fs::read(path).map_err(|_| CliFailure::usage("cannot read CA certificate"))?;
    TlsRootCertificates::from_pem_bundle(&pem)
        .map_err(|_| CliFailure::usage("invalid CA certificate bundle"))
}

pub fn migration_read_clients(
    source_server: &str,
    destination_server: &str,
) -> Result<(ImmichReadClient, ImmichReadClient), CliFailure> {
    let source_endpoint = ImmichEndpoint::parse(source_server)
        .map_err(|_| CliFailure::usage("invalid source origin"))?;
    let destination_endpoint = ImmichEndpoint::parse(destination_server)
        .map_err(|_| CliFailure::usage("invalid destination origin"))?;
    EndpointAccess::disposable(&source_endpoint)
        .map_err(|_| CliFailure::usage("migration source must be disposable loopback"))?;
    EndpointAccess::disposable(&destination_endpoint)
        .map_err(|_| CliFailure::usage("migration destination must be disposable loopback"))?;
    if source_endpoint.same_origin(&destination_endpoint) {
        return Err(CliFailure::usage(
            "migration source and destination origins must differ",
        ));
    }
    let source_value = api_key_value(SOURCE_API_KEY_ENVIRONMENT, SOURCE_API_KEY_FILE_ENVIRONMENT)?;
    let destination_value = api_key_value(
        DESTINATION_API_KEY_ENVIRONMENT,
        DESTINATION_API_KEY_FILE_ENVIRONMENT,
    )?;
    if source_value.as_bytes() == destination_value.as_bytes() {
        return Err(CliFailure::authentication());
    }
    let config = ClientConfig {
        max_response_bytes: 1024 * 1024,
        ..ClientConfig::default()
    };
    let source = ImmichReadClient::new(
        source_endpoint,
        ApiKey::new(&source_value).map_err(|_| CliFailure::authentication())?,
        config,
    )
    .map_err(CliFailure::from_client)?;
    let destination = ImmichReadClient::new(
        destination_endpoint,
        ApiKey::new(&destination_value).map_err(|_| CliFailure::authentication())?,
        config,
    )
    .map_err(CliFailure::from_client)?;
    Ok((source, destination))
}

fn api_key_value(direct_name: &str, file_name: &str) -> Result<String, CliFailure> {
    let direct = std::env::var_os(direct_name);
    let file = std::env::var_os(file_name);
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
