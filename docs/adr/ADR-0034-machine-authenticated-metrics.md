# ADR-0034: Machine-authenticated Web Console metrics

- Status: Accepted
- Date: 2026-09-10
- Owners: project maintainers
- Supersedes clause in: ADR-0033

## Context

ADR-0033 requires every Web Console route, including `/metrics`, to use the
same listener and network policy. The first implementation authorized metrics
only with a browser session. That prevents unattended Prometheus-compatible
scraping and encourages the unsafe practice of copying a browser cookie into a
monitoring system.

The machine path must not create a second listener, weaken direct TLS, trust
forwarded headers, disclose operator state or grant access to any workflow.

## Decision

This ADR supersedes only ADR-0033's browser-session-only metrics clause. An
optional `[web.metrics]` policy may authorize `GET` or `HEAD /metrics` with an
HTTP `Authorization: Bearer` credential. Existing authenticated browser
sessions remain valid for this route.

The metrics credential is exactly 32 random bytes encoded as 64 lowercase
hexadecimal characters. It is loaded at startup from an absolute, bounded,
regular, non-symlink file; Unix permissions must be owner-only. The configured
policy also requires a non-empty, duplicate-free allowlist of at most 32 IPv4
or IPv6 CIDRs. Machine authorization succeeds only when the immediate TCP peer
is in that allowlist and the credential digest matches in constant time.

The credential is accepted only in the single Authorization header. Query
parameters, cookies other than the existing browser session, forwarded
headers, browser fields and environment variables cannot supply it. A bearer
credential never authorizes another route, creates a session or grants an
Immich capability. Invalid and absent credentials receive the same response.
Rotation is an atomic secret-file replacement followed by a controlled restart;
the process does not retain the plaintext after startup.

The endpoint remains on the configured console listener and retains exact Host,
direct TLS in LAN mode, bounded headers/connections/deadlines, no-store response
headers and forwarded-header rejection. Its fixed label-free metric allowlist
is unchanged. Operators should bind the listener and firewall so only the
declared monitoring sources can reach it.

## Consequences

Prometheus-compatible collectors can scrape without borrowing a human session.
Deployment requires one additional secret and an explicit source-network
allowlist. Secret rotation intentionally logs out browser sessions because the
whole console process is restarted.

The credential is a bearer secret, so TLS and network restriction remain
mandatory outside loopback. This mechanism does not add trusted-proxy support,
a separate plaintext listener or remote telemetry.

## Verification

Configuration and secret tests reject relative paths, empty, duplicate or
malformed CIDRs, wrong token format, permissive Unix modes, symlinks, oversized
files and identity changes. HTTP tests prove that missing, malformed, duplicate,
query and wrong bearer values fail; an out-of-policy peer fails; a valid
in-policy bearer receives only the fixed bounded payload; browser sessions
remain valid; and the bearer cannot access any other route.

Container and disposable direct-TLS rehearsals use generated credentials and
synthetic data, verify unattended scraping, then remove the exact secret and
every disposable resource. Production endpoints, credentials, media, live
Authentik and the homelab immich-go smoke remain excluded.
