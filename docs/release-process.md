# Release candidate process

No supported release exists yet. This procedure becomes active only after the
ADR-0025 prerequisites are provisioned.

## One-time prerequisites

1. Retain the existing `docker` Linux x86-64 runner and the manually operated
   `macos-arm64` and `windows-x64` host runners validated by Gitea runs 5440
   and 5470. Register the remaining protected native runners named
   `linux-arm64` and `macos-x64`. Keep host runners offline outside an
   intentional validation or release window.
2. Create the project release OpenPGP identity offline under maintainer
   control. Commit only its armored public key as `docs/release-signing-key.asc`.
3. Store the armored private key, its fingerprint and its passphrase as the
   protected Gitea release secrets `RELEASE_SIGNING_PRIVATE_KEY`,
   `RELEASE_SIGNING_FINGERPRINT` and `RELEASE_SIGNING_PASSPHRASE`. Do not expose
   them to pull requests or ordinary push jobs.
4. Validate all five host targets, repository secrets and runner isolation with
   a non-publishing `release-rehearsal` workflow dispatch. It retains no
   artifact and does not create a tag or release.

The project does not download or redistribute Apple SDKs. Both macOS artifacts
must be built and tested on native Apple hosts. macOS host runners must provide
Python 3.12 as `python3.12`; the workflows deliberately avoid privileged
`actions/setup-python` installation on those runners.

## Candidate checklist

1. Confirm `main` is clean, equal to `origin/main` and green on its exact SHA.
2. Confirm Phase 0–5, both Phase 6 scale, Phase 7 production-HTTPS and both
   Phase 8 Takeout evidence validators pass.
3. Set the Cargo workspace version to the candidate version and update
   `CHANGELOG.md`, compatibility matrices and this guide with verified facts.
4. Create an annotated signed tag such as `v0.1.0-rc.1`; verify it locally with
   `git verify-tag` against the committed public key.
5. Push only the signed tag. The release workflow must build/test all five
   native targets, generate CycloneDX SBOMs, build the attested amd64/arm64 OCI
   archive, create deterministic native archives and provenance, sign
   `SHA256SUMS` and verify the detached signature.
6. Download the workflow artifact into a new directory. Verify the signature,
   every checksum, SBOM schema, provenance source SHA and `immich-rs --version`
   before publishing the immutable candidate.
7. Record the exact tag, workflow run and artifact digests in `ROADMAP.md` and
   project memory. Never replace an artifact under an existing tag.

Any missing native or OCI target, signature, SBOM, provenance statement or
compatibility gate aborts publication. The multiarch archive and its verified
container report are covered by the signed checksum manifest. `latest`
aliases are not release evidence.
