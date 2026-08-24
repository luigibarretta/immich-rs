# Configuration

`immich-rs` resolves configuration in this exact order:

1. command-line option;
2. `IMMICH_RS_*` environment variable;
3. explicit schema-v1 TOML file;
4. bounded built-in default.

Select a file with `--config /path/to/immich-rs.toml` or
`IMMICH_RS_CONFIG`. The CLI selector wins when both are present. The file must
be a non-empty regular UTF-8 file no larger than 1 MiB. Unknown sections,
keys, schema versions and `IMMICH_RS_*` names fail closed.

Run `immich-rs config show` to emit the effective non-secret configuration as
stable JSON. This command never includes an API key.

## TOML schema

```toml
schema_version = 1

[immich]
server = "http://127.0.0.1:2283"
# ca_certificate = "/run/secrets/private-ca.pem"

[scan]
label = "synthetic-source"
source = "/source"
inputs = ["/source/part-001.zip", "/source/part-002.zip"]
buffer_bytes = 65536
max_entries = 100000
max_directory_entries = 10000
max_path_bytes = 4096
case_sensitive = true
max_archives = 64
max_archive_entry_bytes = 1099511627776
max_compression_ratio = 200
compression_ratio_grace_bytes = 1048576

[apple_photos]
album_mode = "none"
album_path_joiner = " - "

[upload]
plan = "/state/upload-plan.json"
source = "/source"
checkpoint = "/state/checkpoint.sqlite"
dry_run = true
verification_buffer_bytes = 65536
concurrency = 1
max_attempts_per_operation = 3
max_retries_per_run = 100
retry_base_delay_ms = 100
retry_delay_cap_ms = 10000

[archive]
selection = "timeline"
include_trashed = false
page_size = 100
max_assets = 100000
manifest = "/state/archive-manifest.json"
destination = "/output"
```

`scan.source` supplies a folder input and is also the one-input fallback for
Takeout or Apple planning. `scan.inputs` supplies the ordered set for an export
adapter, including `plan upload google-takeout` and `plan upload apple-photos`.
Positional CLI inputs replace the complete configured input set.
`upload.source` overrides `scan.source`
only for schema-v1 folder apply. Repeated `--input` values or
`IMMICH_RS_INPUTS_JSON` select the exact directory or split-ZIP set for
schema-v2 Takeout or Apple apply. Apple apply additionally binds
`apple_photos.album_mode` and `apple_photos.album_path_joiner`. Upload execution
limits bind both the server-aware plan and its source-aware execution.

## Environment matrix

| TOML key | Environment | CLI option |
|---|---|---|
| `immich.server` | `IMMICH_RS_SERVER` | `--server` |
| `immich.ca_certificate` | `IMMICH_RS_CA_CERTIFICATE` | `--ca-certificate` |
| `scan.label` | `IMMICH_RS_LABEL` | `--label` |
| `scan.source` | `IMMICH_RS_SOURCE` | positional path / `--source` |
| `scan.inputs` | `IMMICH_RS_INPUTS_JSON` | positional paths |
| `scan.buffer_bytes` | `IMMICH_RS_BUFFER_BYTES` | `--buffer-bytes` |
| `scan.max_entries` | `IMMICH_RS_MAX_ENTRIES` | `--max-entries` |
| `scan.max_directory_entries` | `IMMICH_RS_MAX_DIRECTORY_ENTRIES` | `--max-directory-entries` |
| `scan.max_path_bytes` | `IMMICH_RS_MAX_PATH_BYTES` | `--max-path-bytes` |
| `scan.case_sensitive` | `IMMICH_RS_CASE_SENSITIVE` | `--case-sensitive` / `--case-insensitive` |
| `scan.max_archives` | `IMMICH_RS_MAX_ARCHIVES` | `--max-archives` |
| `scan.max_archive_entry_bytes` | `IMMICH_RS_MAX_ARCHIVE_ENTRY_BYTES` | `--max-archive-entry-bytes` |
| `scan.max_compression_ratio` | `IMMICH_RS_MAX_COMPRESSION_RATIO` | `--max-compression-ratio` |
| `scan.compression_ratio_grace_bytes` | `IMMICH_RS_COMPRESSION_RATIO_GRACE_BYTES` | `--compression-ratio-grace-bytes` |
| `apple_photos.album_mode` | `IMMICH_RS_ALBUM_MODE` | `--album-mode` |
| `apple_photos.album_path_joiner` | `IMMICH_RS_ALBUM_PATH_JOINER` | `--album-path-joiner` |
| `upload.plan` | `IMMICH_RS_UPLOAD_PLAN` | `--plan` |
| `upload.source` | `IMMICH_RS_UPLOAD_SOURCE` | `--source` |
| `upload.checkpoint` | `IMMICH_RS_UPLOAD_CHECKPOINT` | `--checkpoint` |
| `upload.dry_run` | `IMMICH_RS_UPLOAD_DRY_RUN` | `--dry-run` / `--no-dry-run` |
| `upload.verification_buffer_bytes` | `IMMICH_RS_VERIFICATION_BUFFER_BYTES` | `--verification-buffer-bytes` |
| `upload.concurrency` | `IMMICH_RS_CONCURRENCY` | `--concurrency` |
| `upload.max_attempts_per_operation` | `IMMICH_RS_MAX_ATTEMPTS_PER_OPERATION` | `--max-attempts-per-operation` |
| `upload.max_retries_per_run` | `IMMICH_RS_MAX_RETRIES_PER_RUN` | `--max-retries-per-run` |
| `upload.retry_base_delay_ms` | `IMMICH_RS_RETRY_BASE_DELAY_MS` | `--retry-base-delay-ms` |
| `upload.retry_delay_cap_ms` | `IMMICH_RS_RETRY_DELAY_CAP_MS` | `--retry-delay-cap-ms` |
| `archive.selection` | `IMMICH_RS_ARCHIVE_SELECTION` | `--selection` |
| `archive.include_trashed` | `IMMICH_RS_ARCHIVE_INCLUDE_TRASHED` | `--include-trashed` / `--exclude-trashed` |
| `archive.page_size` | `IMMICH_RS_ARCHIVE_PAGE_SIZE` | `--page-size` |
| `archive.max_assets` | `IMMICH_RS_ARCHIVE_MAX_ASSETS` | `--max-assets` |
| `archive.manifest` | `IMMICH_RS_ARCHIVE_MANIFEST` | `--manifest` |
| `archive.destination` | `IMMICH_RS_ARCHIVE_DESTINATION` | `--destination` |

`IMMICH_RS_INPUTS_JSON` is a JSON string array, which avoids ambiguous path
delimiters across Linux, macOS and Windows. Boolean environment values are
exactly `true` or `false`; numeric values are base-10 integers and remain
subject to the command's documented bounds.

## Secrets

API keys are deliberately absent from the TOML schema. Configure exactly one
of:

- `IMMICH_RS_API_KEY` for an ephemeral process environment;
- `IMMICH_RS_API_KEY_FILE` for a regular, non-symlink secret file of at most
  4,097 bytes, with one optional trailing newline.

Configuring both fails with the authentication exit class. The file contents,
path and key are never rendered. Compose deployments should prefer a mounted
secret file. Dry-run does not read either secret source and discards inherited
server configuration before constructing its execution request.

`immich.ca_certificate` adds a bounded PEM trust bundle for a private HTTPS
deployment. It never disables certificate or hostname verification, accepts at
most 64 certificates from a regular non-symlink file of at most 1 MiB, and is
rejected for disposable HTTP mode. Publicly trusted HTTPS endpoints do not need
this option.

Production authorization is intentionally not configurable. The read and write
acknowledgements, exact plan digest, operation budget and verified-backup
reference exist only as explicit CLI options on the invocation that uses them.
They have no TOML or environment equivalents and are not persisted verbatim.
