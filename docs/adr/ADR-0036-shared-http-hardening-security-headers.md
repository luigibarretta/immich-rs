# ADR-0036: Shared http-hardening security headers

- Status: Accepted
- Date: 2026-11-10
- Owners: project maintainers

## Context

ADR-0033 requires every Web Console response to carry a restrictive CSP,
frame denial, `Referrer-Policy: no-referrer`, `X-Content-Type-Options:
nosniff` and private `no-store` caching. The console wrote that set by hand in
`immich-web/src/policy.rs` (`harden_response`, always overwriting). The fleet
library `lib-http-hardening` 0.2.0 provides the same contract once, with
validated values, an overwrite-by-default `SecurityHeadersLayer` for tower and
axum 0.8, and an anti-SSRF outbound guard (`net_guard`, reqwest and ureq
adapters). With only the `tower` feature it adds `http`, `tower-layer`,
`tower-service` and `pin-project-lite`, all already in the dependency graph.

## Decision

immich-rs consumes `http-hardening` 0.2.0 as a vendored path dependency
(`vendor/http-hardening`, an exact `git archive` of the pinned tag recorded in
`dependencies/http-hardening.sha`, without the upstream consumer survey; tree
digest in `dependencies/http-hardening-vendor.sha256`, verified by the
`vendored-deps` ci-local step) with `default-features = false` and only
`tower`. The crate is `MIT OR Apache-2.0`, compatible with ADR-0003.

- `immich-web/src/security_headers.rs` builds the policy once at startup and
  the router applies `SecurityHeadersLayer` as the outermost layer, covering
  the same responses as before (request-policy rejections, limit and timeout
  responses were already hardened). Values are unchanged except one fix: the
  previous CSP (without the library's extra directives) with `form-action`
  extended by the OIDC issuer origin when the LAN profile is active, because
  `POST /oidc/login` answers with a redirect to the identity provider and
  Chromium applies `form-action` to that redirect, which blocked the login; the three-feature Permissions-Policy, COOP and CORP
  `same-origin`, no HSTS, and `Cache-Control`, `Pragma` and `X-Robots-Tag` as
  extra headers. Existing values are overwritten (library default).
- The outbound guard is not adopted. Every outbound target is operator
  configuration: Immich server origins in the server profiles (ADR-0027) and
  the OIDC issuer with its explicit `allowed_cidrs` (ADR-0033). No URL is
  supplied by a console user at runtime. The OIDC client already resolves the
  issuer once, checks every address against the operator allowlist, pins the
  answer with `resolve_to_addrs`, disables redirects, requires HTTPS and
  accepts discovery endpoints only on the issuer origin. It now also disables
  proxies from the environment (`no_proxy`), as does the Immich client for
  pinned endpoints, which closes the remaining gap with the library's
  `PinnedClient` contract (a proxy would resolve the host itself and bypass the
  pinned, allow-listed addresses).

## Consequences

Response header hardening has one implementation shared with the rest of the
fleet, and a value that fails validation stops the console at startup instead
of being sent. Refreshing the library is a pin, tree and digest change
reviewed like any dependency update. Adding HSTS for the direct-TLS LAN
profile or the library's stricter CSP and Permissions-Policy defaults is a
separate, deliberate behaviour change.

## Verification

`check-vendored-dep` proves the vendored tree matches the pin and digest.
`immich-web/tests/security_headers.rs` asserts the exact header set, once
each, on HTML, static assets, the metrics API, redirects and 400, 401, 403,
404, 405 and 413 rejections; `plan_security` asserts it on the JSON plan
export. `cargo deny` checks the dependency's license and the exact-version
path entry.
