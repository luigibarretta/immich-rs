# Security policy

`immich-rs` is not production-ready and currently has no supported release.

Do not report vulnerabilities by opening a public issue containing secrets,
private media, EXIF data or server details. Contact the repository owner
privately through the Gitea account instead.

## Security invariants

- API keys are accepted through protected environment/file/secret-provider
  boundaries and are never emitted in logs, errors, metrics or crash reports.
- TLS verification is enabled by default. Any insecure transport mode must be
  explicit, noisy and unavailable to background automation.
- Media paths and metadata are personal data. Telemetry is local and
  aggregate-only by default.
- Dry-run is enforced below the CLI layer: a dry-run execution cannot reach a
  mutating client method.
- Test fixtures contain no production data.
- Release artifacts require checksums, provenance and an SBOM before the first
  supported version.
