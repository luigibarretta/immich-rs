# Release candidate process

This procedure is active for signed release candidates. A candidate is
published only after the exact `main` revision is green on Gitea and on the
five-target GitHub native matrix.

The current candidate version is `0.1.0-rc.1`. Its signed tag must resolve to
the exact version commit after both main pipelines pass on that commit.

## One-time prerequisites

1. Keep the dedicated OpenPGP private key, fingerprint and passphrase only in
   `immich_rs_release.vault.yml`; commit only
   `docs/release-signing-key.asc` in the product repository.
2. Run `playbooks/reconcile-immich-rs-github-release.yml` from the reviewed
   Ansible repository with its exact confirmation. It proves public/private
   fingerprint equality and writes all three secrets through stdin into the
   `immich-rs-release` GitHub environment.
3. Require a green `native-matrix` run on `ubuntu-24.04`,
   `ubuntu-24.04-arm`, `macos-15-intel`, `macos-15` and `windows-2025` for the
   exact candidate commit. Ordinary main/PR jobs cannot read release secrets.

The project does not download or redistribute Apple SDKs. The `immich-rs` and
`immich-rs-web` artifacts are both built and tested on every target, including
GitHub's native Apple hosted images. Every hosted job installs the same Python
3.12 and Rust 1.88 toolchain contract.

For each product and target, the exact archive basename is
`<product>-<version>-<target>`. Unix and macOS archives use `.tar.gz`; Windows
uses `.zip`. Each has adjacent `.sbom.cdx.json` and `.provenance.json` files.
The OCI basenames are `<product>-<version>-linux-multiarch` with `.oci.tar` and
`.container.json`. The two products share one signed `SHA256SUMS`; no artifact
is replaced under an existing tag.

## Candidate checklist

1. Confirm `main` is clean, equal to `origin/main` and green on its exact SHA.
2. Confirm Phase 0–5, both Phase 6 scale, Phase 7 production-HTTPS and both
   Phase 8 Takeout evidence validators pass.
3. Set the Cargo workspace version to the candidate version and update
   `CHANGELOG.md`, compatibility matrices and this guide with verified facts.
4. Create an annotated signed tag such as `v0.1.0-rc.1`; verify it locally with
   `git verify-tag` against the committed public key.
5. Push only the signed tag. The release workflow must build/test all five
   native targets for both products, generate separate CycloneDX SBOMs, build
   both attested amd64/arm64 OCI archives, create deterministic native archives
   and provenance, sign
   `SHA256SUMS` and verify the detached signature.
6. Download the workflow artifact or GitHub prerelease into a new directory.
   Verify the signature, every checksum, SBOM schema, provenance source SHA and
   `immich-rs --version` and `immich-rs-web --version`; the workflow publishes
   only after doing the same checksum and signature verification itself.
7. Record the exact tag, workflow run and artifact digests in `ROADMAP.md` and
   project memory. Never replace an artifact under an existing tag.

Any missing product, native or OCI target, signature, SBOM, provenance statement
or compatibility gate aborts publication. Both multiarch archives and their
verified container reports are covered by the signed checksum manifest.
`latest` aliases are not release evidence.
