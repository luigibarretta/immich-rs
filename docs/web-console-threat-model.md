# Web Console threat model

This threat model applies to the optional operator console accepted by
ADR-0033. It does not widen the CLI or production-migration surface.

## Protected assets and trust boundaries

Protected assets are Immich credentials, OIDC secrets/tokens, local media and
metadata, source paths, immutable plans, checkpoints, backup references,
operator sessions and server mutation authority. The relevant boundaries are:

- untrusted browser input to authenticated HTTP routes;
- browser-visible summaries to authoritative server-side artifacts;
- configured opaque profiles to local filesystem and outbound HTTPS resources;
- web job admission to the application facade and executor;
- in-memory sessions/grants to separate durable history/checkpoint stores;
- direct TLS clients to the public-origin policy. Trusted proxy termination is
  explicitly unsupported in the implemented LAN slice.

Loopback clients are untrusted until paired and authenticated. Source contents,
archive names, sidecars, IdP responses, Immich responses, DNS answers and proxy
headers are untrusted even after authentication.

## Threats and controls

| Threat | Required controls | Failure behavior |
|---|---|---|
| Unauthenticated local process | Local bootstrap secret, one-time pairing, opaque rotated session | No source, plan, history, server or job access |
| Cross-site request | Exact Host/public Origin, strict Origin and CSRF on every state change, no mutation GET | Reject before route action |
| Session theft/fixation | HttpOnly protected cookie, rotation on login/pairing, idle/absolute expiry, restart logout | Revoke session and its unused grants |
| Stored/reflected XSS | Contextual SSR escaping, no raw HTML/`innerHTML`/inline handlers/external assets, restrictive CSP | Render escaped text or reject oversized input |
| Path traversal or symlink/reparse swap | Opaque configured profiles, canonical containment for every directory/ZIP input, reopen/revalidate identity at use | Fail before source/state access |
| SSRF, redirects or DNS rebinding | Profile-only exact hostnames, literal-loopback disposable targets, HTTPS for remote reads, redirects forbidden, bounded DNS with every answer inside an explicit per-profile CIDR policy, pinned connections with TLS hostname verification | No outbound connection |
| Proxy spoofing | Direct TLS only; reject every forwarded header and require the exact configured Host/Origin | Reject request |
| Credential disclosure | Secret files only, redacting types, no browser storage/history/logs, private state permissions | Fail closed and emit safe diagnostic |
| Plan/count/identity substitution | Canonical server-side digest, source/config/server/user/profile/credential/count binding | Grant creation/admission refused |
| Confirmation replay/race | Single-use in-memory grant consumed atomically with idempotent job admission; bounded spent-receipt tracking | Same job returned or request refused |
| Restart/logout/cancel ambiguity | Unused grants invalidated; admitted run is executor-owned and checkpointed; checkpoint time and process-local receipt use require a later dry-run before resume | Never reconstruct authority |
| Slow client or SSE disconnect | Bounded replay/subscribers, owned tasks, polling fallback; disconnect does not cancel | Drop subscriber, preserve job |
| Resource exhaustion | Numerical HTTP/session/job/SSE/history/plan limits and backpressure | Reject admission or stop cleanly |
| History leakage/corruption | Separate strict minimal schema, enum/counter-only typed writes, no identifiers/secrets/metadata, private file, transactional migration, bounded pages/WAL/retention and authenticated reads | Refuse newer/corrupt/full store and stop further job admission after a terminal-write failure |
| OIDC mix-up/replay | Code+PKCE, exact issuer/audience/redirect/signature/alg/exp, single-use state/nonce, role/subject mapping | No session issued |
| JWKS rotation/outage abuse | Bounded fresh discovery/JWKS fetch and key set for login and callback; fail closed on outage or unknown/invalid keys | Login refused |
| Excess privilege | Authz on every object/stream/export/metrics route; executor remains sole effect owner | Deny without disclosing existence |
| Sensitive metrics | Aggregate low-cardinality allowlist, protected listener, forbidden-label tests | Metric omitted or request denied |

The implemented metrics surface uses only nine fixed names and no labels. It
shares the console listener, direct-TLS/OIDC or loopback-session boundary,
exact Host validation and private no-store response policy. A dedicated
machine bearer, unauthenticated scrape path and forwarded-header exception are
not implemented.

## Explicitly excluded validation targets

Tests must not use a production endpoint or credential, personal media, live
Authentik, or the homelab immich-go periodic smoke. Permitted targets are
synthetic fixtures, loopback mocks, disposable Immich, and disposable IdP/TLS
environments. Every disposable resource and sensitive temporary artifact is
removed on all exits.
