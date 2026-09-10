# Web Console operator and recovery guide

This guide covers the optional `immich-rs-web` operator console accepted by
ADR-0033. It is not a gallery, media server, general backup system or production
migration interface. The CLI remains the complete canonical interface.

## Start and authenticate

Use one strict operator-owned TOML selected by `--config` or
`IMMICH_RS_WEB_CONFIG`. Create the configured state root with private ownership
before startup, and mount sources, configuration, API keys, TLS identity and
OIDC material read-only. Never put a secret, absolute path or server URL into a
browser field. The [configuration guide](configuration.md) contains a minimal
loopback example; the [LAN guide](web-console-lan.md) defines direct TLS and
OIDC.

Loopback startup reads a 16–256 byte private bootstrap file. Pair once before
its bounded lifetime expires; pairing consumes the bootstrap authority and
rotates to an opaque in-memory session. A restart requires pairing again. LAN
mode has no bootstrap and refuses startup unless the complete TLS/OIDC policy is
valid. Logout and session expiry revoke unused grants and SSE access.

For each plan, inspect the server-rendered summary, export the immutable plan if
needed, complete the mandatory offline dry-run, then separately retype its
exact digest and logical-effect maximum. Production apply also requires the raw
reference to an independently verified restore point. The raw reference, grant,
session and OIDC tokens are never durable. Cancellation or restart requires a
new login, dry-run receipt and confirmation before checkpoint resume.

## Retention and capacity

Terminal history is a separate `console-history-v1.sqlite3` database. Defaults
retain 5,000 rows for 30 days in at most 32 MiB including WAL; hard maxima are
10,000 rows, 90 days and 64 MiB. Writes evict oldest rows transactionally and
checkpoint the bounded WAL. History contains only workflow/status enums, safe
counters, opaque plan references and versioned dry-run bindings. It is not an
executor checkpoint and cannot authorize work.

In-memory jobs, queues, sessions and SSE replay/subscribers use the exact bounds
in the [resource matrix](web-console-resources.md) and disappear at restart.
Immutable plan files default to 128 MiB each and 512 MiB in aggregate; the hard
maxima are 256 MiB and 4 GiB. Apply checkpoints and plans persist because they
may be required for safe convergence. This version has no in-console garbage
collector for them. Reaching a state or disk bound refuses new work without
truncating an artifact.

For safe rollover, stop the console, retain a protected archive of the complete
old state root, configure a new empty private state root, increment the state
generation, and restart. Do not merge state roots or delete individual plan,
binding or checkpoint files while the console is running. Rollover intentionally
ends access to old history and resume state.

## Backup and restore

The console state is sensitive even though it contains no active tokens. Plans
and checkpoints can reveal normalized metadata and operation state. Encrypt
operator backups, restrict their ownership and never publish them as CI
artifacts.

1. Stop `immich-rs-web` cleanly and verify the exact process or Compose service
   has exited. Do not copy a live SQLite database.
2. Copy the complete configured state root as one permission-preserving unit,
   including `console-history-v1.sqlite3`, any `-wal`/`-shm` companions,
   `plans/` and `checkpoints/`. Back up operator TOML, secrets, TLS/OIDC material
   and source data through their separate owner-approved procedures.
3. Restore into an empty private directory owned by the console identity. On
   Unix, keep directories owner-only and files owner-readable/writable only.
   Never combine artifacts from different state roots.
4. Point one state profile at the restored root with the intended generation
   and start the same or a newer compatible binary. Startup validates identity,
   containment, permissions, plan bindings, SQLite integrity and schema.
5. Pair or authenticate again. Sessions and grants are intentionally absent.
   Before any checkpoint resume, inspect the plan and complete a fresh offline
   dry-run plus exact confirmation.

A newer-than-supported history schema, corruption, symlink, wrong permissions,
identity swap or capacity breach fails closed. Do not bypass the check or remove
the WAL alone. Stop the console, preserve the failed root for diagnosis, then
restore the last known-good complete root or start a new empty root. After disk
exhaustion, stop the process, restore sufficient capacity, preserve the complete
state set and retry startup; a missing terminal-history write prevents successful
job publication.

## Rotation and drift

- Rotate an Immich API key by replacing its private secret file through the
  operator secret workflow and incrementing `credential_generation`.
- Increment a source, state or server generation after changing its roots,
  inputs, adapter options, address CIDRs, origin or CA path. Old plans, receipts
  and unused grants must fail rather than cross generations.
- Rotate TLS certificates, private keys or OIDC client material while stopped,
  validate ownership and restart. Restart logs out every session.
- Rotate the optional metrics bearer by atomically replacing its private file
  while stopped, then restart. Update the collector through its secret workflow;
  never log, paste into TOML or pass the value on a command line.
- OIDC signing-key rotation is discovered during a fresh bounded login. Unknown
  keys, mixed/out-of-policy DNS answers and provider outage fail authentication
  closed.

## Metrics and health interpretation

Authenticated `GET /metrics` publishes nine label-free gauges listed in the
[resource matrix](web-console-resources.md). The endpoint shares the console
Host, TLS and no-forwarded-header policy. A browser session remains supported,
but unattended scraping is supported only when `[web.metrics]` supplies a
private file-backed bearer and the collector's immediate peer matches an
explicit CIDR. Never export a browser cookie to a general metrics system.

Configure the collector to send the bearer from its own protected credential
file, over the existing direct-TLS listener. Do not put the credential in the
URL. A 401 indicates missing/invalid authorization or a peer outside the CIDR
policy; confirm the address observed after container or host networking rather
than broadening the range. A 503 means the relevant lock or history store could
not be read. The bearer grants no dashboard or mutation access.

`jobs_queued` and `jobs_running` show current bounded admission; terminal job
gauges count only records retained in memory. `history_rows` is the current
durable row count, while `sessions_active` and `sse_subscribers` are
process-local. These are state gauges, not cumulative success or service-level
metrics. Investigate state integrity and capacity without weakening
authentication.

## Regression and shutdown gates

Run the deterministic SSR/security checks in the normal workspace gate. When a
local Chrome-compatible browser is explicitly available, build the exact binary
and run:

```bash
cargo build --locked --release -p immich-rs-web --bin immich-rs-web
scripts/test-web-browser.sh \
  --binary target/release/immich-rs-web \
  --browser /absolute/path/to/google-chrome
```

The browser gate creates only a synthetic source and loopback console, disables
background browser networking, denies hostname resolution except
`127.0.0.1`, verifies semantic and safe DOM markers, captures a temporary PNG,
then removes its exact process, browser profile, secret and workspace. It never
uses production endpoints, personal media, live Authentik or immich-go.

On normal SIGINT, shutdown requests cooperative cancellation from every
executor-owned worker and joins each worker before exit. An SSE or browser
disconnect alone never cancels a job. Use the authenticated cancel action when
cancellation is intended; inspect the terminal state before stopping the
service.
