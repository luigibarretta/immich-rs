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
- direct TLS clients or a declared trusted proxy to the public-origin policy.

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
| Path traversal or symlink/reparse swap | Opaque configured profiles, canonical containment, reopen/revalidate identity at use | Fail before source/state access |
| SSRF, redirects or DNS rebinding | Profile-only URLs, HTTPS except disposable loopback, no redirects/embedded credentials, bounded DNS and address policy | No outbound connection |
| Proxy spoofing | Reject forwarded headers unless peer is explicitly trusted; exact reconstructed origin | Reject request/startup |
| Credential disclosure | Secret files only, redacting types, no browser storage/history/logs, private state permissions | Fail closed and emit safe diagnostic |
| Plan/count/identity substitution | Canonical server-side digest, source/config/server/user/profile/credential/count binding | Grant creation/admission refused |
| Confirmation replay/race | Single-use in-memory grant consumed atomically with idempotent job admission | Same job returned or request refused |
| Restart/logout/cancel ambiguity | Unused grants invalidated; admitted run is executor-owned and checkpointed; resume needs new dry-run | Never reconstruct authority |
| Slow client or SSE disconnect | Bounded replay/subscribers, owned tasks, polling fallback; disconnect does not cancel | Drop subscriber, preserve job |
| Resource exhaustion | Numerical HTTP/session/job/SSE/history/plan limits and backpressure | Reject admission or stop cleanly |
| History leakage/corruption | Separate minimal schema, no identifiers/secrets/metadata, private file, transactional migration, bounded WAL | Refuse newer/corrupt/full store |
| OIDC mix-up/replay | Code+PKCE, exact issuer/audience/redirect/signature/alg/exp, single-use state/nonce, role/subject mapping | No session issued |
| JWKS rotation/outage abuse | Bounded fetch/cache/key set; fail closed on unknown/invalid keys | Existing policy expires; login refused |
| Excess privilege | Authz on every object/stream/export/metrics route; executor remains sole effect owner | Deny without disclosing existence |
| Sensitive metrics | Aggregate low-cardinality allowlist, protected listener, forbidden-label tests | Metric omitted or request denied |

## Explicitly excluded validation targets

Tests must not use a production endpoint or credential, personal media, live
Authentik, or the homelab immich-go periodic smoke. Permitted targets are
synthetic fixtures, loopback mocks, disposable Immich, and disposable IdP/TLS
environments. Every disposable resource and sensitive temporary artifact is
removed on all exits.
