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

[picasa]
albums = false
filename_date = false

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

[migration]
source_server = "http://127.0.0.1:2284"
destination_server = "http://127.0.0.1:2285"
page_size = 100
max_assets = 100000
max_albums = 10000
max_album_memberships = 1000000
max_asset_bytes = 1099511627776
max_total_bytes = 1099511627776
plan = "/state/migration-plan.json"
checkpoint = "/state/migration-checkpoint.sqlite"
dry_run = true
```

`scan.source` supplies a folder input and is also the one-input fallback for
Takeout or Apple planning. `scan.inputs` supplies the ordered set for an export
adapter, including `plan upload google-takeout` and `plan upload apple-photos`.
Positional CLI inputs replace the complete configured input set.
`upload.source` overrides `scan.source`
only for schema-v1 folder apply. Repeated `--input` values or
`IMMICH_RS_INPUTS_JSON` select the exact directory or split-ZIP set for
schema-v2 Takeout or Apple apply. Apple apply additionally binds
`apple_photos.album_mode` and `apple_photos.album_path_joiner`. Picasa planning
and apply similarly bind `picasa.albums` and `picasa.filename_date`. Upload
execution limits bind both the server-aware plan and its source-aware execution.

The migration section binds two distinct servers, inventory/byte ceilings,
the immutable `migration-plan-v1` and its `checkpoint-v2` journal. The current
migration capability accepts distinct disposable loopback origins only. Its
dry-run discards both server origins and reads neither migration credential.

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
| `picasa.albums` | `IMMICH_RS_PICASA_ALBUMS` | `--picasa-albums` / `--no-picasa-albums` |
| `picasa.filename_date` | `IMMICH_RS_PICASA_FILENAME_DATE` | `--filename-date` / `--no-filename-date` |
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
| `migration.source_server` | `IMMICH_RS_MIGRATION_SOURCE_SERVER` | `--source-server` |
| `migration.destination_server` | `IMMICH_RS_MIGRATION_DESTINATION_SERVER` | `--destination-server` |
| `migration.page_size` | `IMMICH_RS_MIGRATION_PAGE_SIZE` | `--page-size` |
| `migration.max_assets` | `IMMICH_RS_MIGRATION_MAX_ASSETS` | `--max-assets` |
| `migration.max_albums` | `IMMICH_RS_MIGRATION_MAX_ALBUMS` | `--max-albums` |
| `migration.max_album_memberships` | `IMMICH_RS_MIGRATION_MAX_ALBUM_MEMBERSHIPS` | `--max-album-memberships` |
| `migration.max_asset_bytes` | `IMMICH_RS_MIGRATION_MAX_ASSET_BYTES` | `--max-asset-bytes` |
| `migration.max_total_bytes` | `IMMICH_RS_MIGRATION_MAX_TOTAL_BYTES` | `--max-total-bytes` |
| `migration.plan` | `IMMICH_RS_MIGRATION_PLAN` | `--plan` |
| `migration.checkpoint` | `IMMICH_RS_MIGRATION_CHECKPOINT` | `--checkpoint` |
| `migration.dry_run` | `IMMICH_RS_MIGRATION_DRY_RUN` | `--dry-run` / `--no-dry-run` |

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

Immich-to-Immich migration uses two separate secret pairs instead:

- `IMMICH_RS_SOURCE_API_KEY` or `IMMICH_RS_SOURCE_API_KEY_FILE`;
- `IMMICH_RS_DESTINATION_API_KEY` or `IMMICH_RS_DESTINATION_API_KEY_FILE`.

Exactly one member of each pair is required for migration planning and live
apply. The two key values must differ. Migration dry-run reads none of them;
the source client type exposes no mutation method, while only the destination
client can be upgraded to the plan-bound import capability.

`immich.ca_certificate` adds a bounded PEM trust bundle for a private HTTPS
deployment. It never disables certificate or hostname verification, accepts at
most 64 certificates from a regular non-symlink file of at most 1 MiB, and is
rejected for disposable HTTP mode. Publicly trusted HTTPS endpoints do not need
this option.

Production authorization is intentionally not configurable. The read and write
acknowledgements, exact plan digest, operation budget and verified-backup
reference exist only as explicit CLI options on the invocation that uses them.
They have no TOML or environment equivalents and are not persisted verbatim.

## Web Console configuration namespace

`immich-rs-web` uses a separate strict schema-v1 TOML. Select it with
`--config /absolute/path/to/immich-rs-web.toml` or, for the standalone binary,
`IMMICH_RS_WEB_CONFIG`; the command-line selector wins. No other
`IMMICH_RS_WEB_*` environment variable is accepted. Unknown keys, sections,
profile kinds and schema versions fail startup closed, and the bounded config
file must be regular and non-symlink.

This minimal loopback configuration enables authenticated folder scan/review
without constructing a server client:

```toml
schema_version = 1

[web]
listen_address = "127.0.0.1:2285"
public_origin = "http://127.0.0.1:2285"
bootstrap_secret_file = "/run/secrets/console-bootstrap"
history_state_id = "console"

[[sources]]
id = "camera-roll"
label = "Camera roll"
allowed_root = "/sources"
relative_root = "."
generation = 1

[[states]]
id = "console"
label = "Console state"
allowed_root = "/state"
relative_root = "."
generation = 1
```

The bootstrap value is a private regular UTF-8 file containing 16–256 bytes,
with at most one trailing line ending. On Unix, bootstrap, API-key, OIDC-client
and TLS-private-key files require owner-only permissions; the state root
requires private directory permissions. Browser requests carry only opaque
profile IDs. Absolute source/state paths, secret paths, server origins and
adapter limits remain operator-owned TOML.

A folder profile uses exactly one contained `relative_root`. A
`google_takeout`, `apple_photos` or `picasa` profile instead uses one contained
directory or a bounded `relative_inputs` array of contained ZIP files. Its
`[sources.scan]`, `[sources.options]` and `[sources.upload]` values form the
source generation binding. Server profiles are either literal-loopback
`disposable` targets or exact-hostname HTTPS `production_read` targets with an
explicit private/public IPv4/IPv6 CIDR policy, credential generation and
optional private CA file. See the
[capability/resource matrix](web-console-resources.md) and
[direct-TLS LAN example](web-console-lan.md) for the complete boundaries.

Increment the affected source, state, server or `credential_generation` value
whenever its operator-owned input changes. Existing plans, receipts and unused
grants then fail binding checks; do not reuse a generation number to conceal
drift. Production confirmation remains an authenticated, short-lived browser
action bound to an already completed dry-run, never a TOML or environment
setting.
