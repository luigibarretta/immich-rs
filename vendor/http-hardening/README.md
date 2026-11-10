# lib-http-hardening

Shared Rust HTTP hardening for personal projects: security response headers
and an anti-SSRF guard for outbound requests.

The repository uses the `lib-` prefix to identify its role in the catalog.
The Cargo package and Rust crate are `http-hardening` / `http_hardening`.
It was extracted in phase 3 of the 2026-11-07 fleet audit
(`~/audits/2026-11-07-fleet/_PIANO-moduli-condivisi.md`). The consumer survey
and adoption notes are in
[`docs/consumer-survey-2026-11-09.md`](docs/consumer-survey-2026-11-09.md).

## Modules and features

| Item | Feature | Dependencies |
|------|---------|--------------|
| `security_headers` (string core) | always | none (std only) |
| `SecurityHeaders::headers()` / `apply(&mut HeaderMap)` | `http` (default) | `http` 1.x |
| `security_headers::SecurityHeadersLayer` | `tower` | `http`, `tower-layer`, `tower-service`, `pin-project-lite` |
| `http_hardening::net_guard` | `net-guard` (default) | `url` 2.5 |
| `net_guard::reqwest_adapter` | `reqwest` | `net-guard`, `reqwest` 0.12 (no default features), `tokio` (`net`) |
| `GuardedRequest::json`, `form`, `query` | `reqwest-serde` | `reqwest` + reqwest `json`, `serde` |
| `net_guard::ureq_adapter` | `ureq` | `net-guard`, `ureq` 2.12 (no default features) |

`default-features = false` gives a dependency-free crate (oragate): render
`SecurityHeaders::headers_str()` into the hand-written response.

**TLS:** reqwest and ureq are pulled in with `default-features = false`, so
this crate enables no TLS backend. Consumers that call `https` URLs must
enable one on their own dependency (e.g. `reqwest = { version = "0.12",
features = ["rustls-tls"] }`, `ureq = { version = "2.12", features =
["tls"] }`); Cargo feature unification then applies it to the adapters.

## `security_headers`

```rust
use http_hardening::security_headers::{
    ContentSecurityPolicy, FrameOptions, SecurityHeaders, SecurityHeadersLayer,
};

let csp = ContentSecurityPolicy::strict()
    .add("connect-src", "wss://push.example")?
    .add("script-src", "'sha256-<base64 of the inline bootstrap script>'")?;
let headers = SecurityHeaders::builder()
    .csp(Some(csp))
    .frame_options(FrameOptions::SameOrigin)
    .header("Cache-Control", "no-store")
    .keep_if_present(&["content-security-policy"]) // e.g. a sandboxed preview route
    .build()?;
let app = router.layer(SecurityHeadersLayer::new(headers)); // feature `tower`
```

Strict defaults: HSTS two years with `includeSubDomains`; a deny-by-default
CSP (`default-src 'none'`, same-origin scripts, styles, images, fonts,
connections, manifest and workers, `base-uri 'none'`, `object-src 'none'`);
`X-Frame-Options: DENY` plus `frame-ancestors 'none'`;
`X-Content-Type-Options: nosniff`; `Referrer-Policy: no-referrer`; a
`Permissions-Policy` denying camera, microphone, geolocation, payment, USB and
similar; COOP and CORP `same-origin`.

- Policy values **overwrite** anything the handler or an upstream already
  set (`ExistingHeaders::Overwrite`, the default), so a reverse-proxied
  upstream cannot weaken them. `keep_if_present(&[...])` lets single headers
  keep a present value; `ExistingHeaders::Keep` keeps all.
- HSTS `includeSubDomains` makes browsers refuse plain HTTP on every
  subdomain of the sending host: if an HTTP-only LAN device lives under the
  same parent (e.g. `printer.lan.example`), send
  `Hsts::default().include_subdomains(false)` there, or `hsts(None)` for a
  plain-HTTP service.
- `form-action` is not in the strict CSP: Chrome applies it to the redirect
  of a login POST to an external IdP. Add `.add("form-action", "'self'")`
  (plus the IdP origin) where no such flow exists.
- No CSP nonces: for a fixed inline script add its hash
  (`openssl dgst -sha256 -binary s.js | openssl base64`) as
  `'sha256-...'`, or move the script to a file.
- `FrameOptions` drives both `X-Frame-Options` and CSP `frame-ancestors`; a
  `frame-ancestors`-only CSP is still sent when the CSP is disabled.
- CSP sources, directive names, Permissions-Policy origins and extra headers
  are validated (no whitespace, `;`, `,`, quotes, backslashes or parentheses
  where they could break out) and bounded.

## `net_guard`

```rust
use std::sync::Arc;
use http_hardening::net_guard::{NetGuard, RedirectPolicy, Scheme};
use http_hardening::net_guard::reqwest_adapter::{GuardedClient, PinnedClient};

let guard = Arc::new(
    NetGuard::builder()
        .allow_scheme(Scheme::Http)                     // LAN ntfy without TLS
        .allow_cidr_list(&settings.ntfy_allowed_cidrs)? // e.g. "192.168.1.0/24"
        .redirects(RedirectPolicy::revalidate(3))
        .build(),
);
// Configuration time: static checks, no DNS.
let url = guard.parse_url(&settings.webhook_url)?;
// Request time, option A: every request URL checked, DNS checked per hop.
let client = GuardedClient::new(Arc::clone(&guard))?;
client.post(&url)?.json(&payload).send().await?; // json: feature `reqwest-serde`
// Option B: resolve once, connect only to the checked addresses.
let target = guard.resolve(&url)?; // blocking; see check_addresses for async
let pinned = PinnedClient::new(guard, target)?;
pinned.request_target(reqwest::Method::POST).json(&payload).send().await?;
```

Rules (fail closed):

- schemes: `https` by default, `http`, `ws`, `wss` on request; URLs with
  credentials are refused; port 0 is refused and an optional port allowlist
  applies;
- literal hosts are canonicalised by the WHATWG URL parser, so decimal, octal,
  hex and short IPv4 forms (`2130706433`, `0177.0.0.1`, `0x7f.1`, `127.1`,
  percent-encoded and full-width digits) are judged as the address they mean;
- refused classes: unspecified, loopback, RFC 1918, CGNAT, link-local (cloud
  metadata), IETF protocol assignments, documentation, benchmarking,
  multicast, broadcast, reserved, ULA, site-local, discard-only, IPv6 outside
  `2000::/3`;
- IPv6 encodings of IPv4: IPv4-mapped is unwrapped and judged as IPv4;
  well-known NAT64 `64:ff9b::/96` is judged by the embedded IPv4 (public
  NAT64 targets are allowed); IPv4-compatible, local-use NAT64
  `64:ff9b:1::/48`, 6to4 `2002::/16` and Teredo `2001::/32` are refused;
- `localhost` and `*.localhost` are judged as `127.0.0.1` before any DNS;
- DNS is queried once; every answer address must pass (a mixed public and
  private answer is refused as a whole) and at most 32 addresses are
  accepted; the checked addresses are returned (`ResolvedTarget`);
- the explicit allowlist (`Cidr`, `parse_cidr_list`) admits intended
  internal targets: host bits must be zero, IPv4-mapped networks are
  refused, prefixes shorter than /8 (IPv4) or /32 (IPv6) need
  `allow_wide_cidr`, more than 256 entries is an error; unspecified,
  multicast and broadcast are never admissible, and NAT64 encodings do not
  match IPv4 entries. Allowlisting `127.0.0.0/8` admits the `localhost`
  name, but `localhost` usually also resolves to `::1`: list `::1/128` too,
  or the whole answer is refused;
- redirects: `RedirectPolicy::Disabled` (default, the 3xx is returned) or
  `Revalidate { max_hops }` (at most 10) re-checking each hop and refusing
  https to http downgrades.

Clients (correct by construction, inner client not exposed):

- `reqwest_adapter::GuardedClient`: `request(Method, &Url)` returns a
  `GuardedRequest` (header, headers, bearer_auth, body, timeout, send; json,
  form, query with `reqwest-serde`) that never exposes the inner client or a
  mutable URL (`build_split`, `url_mut` are out of reach); it runs
  `check_url` first (literal IP hosts never reach reqwest's resolver; ports
  are enforced here), then `GuardedResolver` checks every DNS answer and the
  redirect policy every hop. No proxy; `https_only` unless HTTP is allowed;
  `with_config` customises timeouts/TLS but cannot override these.
- `reqwest_adapter::PinnedClient`: pinned to a `ResolvedTarget`; refuses
  URLs whose host or port differ (hyper-util would otherwise dial the pinned
  address on the request's port).
- `ureq_adapter::GuardedAgent` (blackstart): `request`/`call`/`send` run
  `check_url`; the agent has `redirects(0)`, `try_proxy_from_env(false)` and
  the validating `UreqResolver`, and `send` follows 3xx itself through
  `check_redirect`. 301/302/303 continue as a plain `GET` (`HEAD` stays
  `HEAD`) without the caller's headers or body; 307/308 replay the caller's
  closure only to the same origin, and a cross-origin 307/308 is returned
  unfollowed so credentials never reach another origin.
- `*_unchecked` builders and `pinned_client_builder` remain for raw access
  (hidden from rustdoc); they do not check the first URL, and a `.proxy()`
  added to them re-enables proxying and bypasses the guard.
- Timeouts apply per request: a redirect chain can take up to
  `(max_hops + 1) * timeout`.

## Rules

- Rust 2021, MSRV 1.85; `unsafe` is forbidden; `unwrap`, `expect` and
  `panic!` are denied outside tests (`[lints]` in `Cargo.toml`, `clippy.toml`).
- Loops are bounded by explicit limits; files stay under 300 lines (fleet
  limit 400, NASA warning target 300).
- Comments and documentation are in English.

## Local gate

Heavy builds on dev-01 go through the build-lock wrapper, one at a time:

```sh
export PATH=$HOME/.local/dev-build-lock-bin:$HOME/.cargo/bin:$PATH
export CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$PWD/target
node third_party/repo-tooling/ci-local.mjs --quick   # needs REPO_TOOLING_UPSTREAM
node third_party/repo-tooling/ci-local.mjs           # full gate
```

## License

MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).
