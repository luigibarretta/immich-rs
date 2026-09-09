# Web Console direct-TLS LAN configuration

The implemented LAN authentication boundary is available through the
`immich-rs-web` library and standalone binary. A separate local OCI/Compose
service is available from an exact checkout; no release image or native Web
Console archive has been published yet.

LAN mode is mutually exclusive with loopback bootstrap pairing. A non-loopback
listen address requires all of this operator-owned configuration:

```toml
[web]
listen_address = "0.0.0.0:2285"
public_origin = "https://console.example.internal:2285"
history_state_id = "console"

[web.lan]
tls_certificate_file = "/run/secrets/console-chain.pem"
tls_private_key_file = "/run/secrets/console-key.pem"

[web.lan.oidc]
issuer = "https://idp.example.internal/application/o/immich-rs/"
client_id = "immich-rs-web"
client_secret_file = "/run/secrets/oidc-client-secret"
ca_certificate_file = "/run/secrets/private-ca.pem"
allowed_cidrs = ["192.0.2.0/24", "fd42:20:30::/64"]
required_role = "immich-rs-operator"
allowed_algorithm = "EdDSA"
```

Use either `required_role` or a non-empty `allowed_subjects` array, never both.
Role mapping reads the bounded `roles` array from the signed ID token. The OIDC
provider must advertise authorization-code, S256 PKCE and EdDSA ID tokens. The
issuer and every advertised endpoint must use the same exact HTTPS origin.

`allowed_cidrs` is an explicit per-provider address policy, not a public-address
filter, so private LAN/VPN identity providers are supported. Before each login
and callback the bounded DNS answer is deduplicated; every address must be
allowed, and the set is pinned into the HTTPS client. Redirects are forbidden
and TLS hostname verification remains enabled. A mixed or changed answer
outside policy fails before HTTP is sent.

The TLS private key and OIDC client secret must be bounded regular files with
owner-only permissions on Unix. Symlinks, identity changes while reading,
multiple private keys, mismatched certificate/key pairs and oversized files
fail startup or login closed. The browser receives no client secret, access
token, refresh token or ID token.

The console currently terminates TLS directly and supports TLS 1.3 only.
Forwarded headers are rejected in every mode; do not place this version behind
a TLS-terminating reverse proxy. The public-origin Host and every state-changing
Origin must match exactly. OIDC sessions are memory-only, rotated after callback,
bounded by idle and absolute lifetimes, and invalidated on restart. Rotation or
logout also revokes unused grants and open SSE streams; a browser/SSE disconnect
does not cancel admitted work.

Tests use a synthetic source, a disposable loopback TLS IdP and generated test
certificates. Live Authentik, production endpoints, production credentials and
personal media are excluded.

Start the native binary with an explicit strict configuration selector:

```bash
IMMICH_RS_WEB_CONFIG=/absolute/path/to/immich-rs-web.toml \
  immich-rs-web
```

`--config PATH` has precedence over `IMMICH_RS_WEB_CONFIG`. Any other
`IMMICH_RS_WEB_*` variable fails closed. Secrets, certificates, source paths,
state paths and server origins remain inside the operator-owned TOML and are
never accepted from the browser.
