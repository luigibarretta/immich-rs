# http-hardening provenance

The `http-hardening` source is vendored so the CLI, the Web Console, both
container images and the public mirror build from one checkout without
cloning a private repository. It is an exact `git archive` of
`lib-http-hardening` tag `v0.2.0`, the revision recorded in
`dependencies/http-hardening.sha` (`be757cf18f5986c3066fdb60499f5bfce0c61d8c`),
with zero local patches. Only the upstream consumer survey
(`docs/consumer-survey-2026-11-09.md`, fleet-internal notes) is left out; it
and any crate `Cargo.lock` are listed under `exclude` in
`dependencies/vendored.json`. The crate is licensed `MIT OR Apache-2.0`,
compatible with AGPL distribution (ADR-0003). immich-rs enables only the
`tower` feature (`SecurityHeadersLayer` on the Web Console router, see
`crates/immich-web/src/security_headers.rs` and ADR-0036).

`dependencies/vendored.json` declares this copy. The `vendored-deps` step
(`check-vendored-dep`) verifies the recorded tree digest in
`dependencies/http-hardening-vendor.sha256` and, when a clone of
`lib-http-hardening` is available (`.ci/lib-http-hardening` or
`../lib-http-hardening`), compares every file with the pinned commit.

To refresh:

```bash
rm -rf vendor/http-hardening && mkdir vendor/http-hardening
git -C ../lib-http-hardening archive <commit> | tar -x -C vendor/http-hardening
rm -f vendor/http-hardening/docs/consumer-survey-*.md
git -C ../lib-http-hardening rev-parse '<tag>^{commit}' > dependencies/http-hardening.sha
node third_party/repo-tooling/check-vendored-dep.mjs --only http-hardening --update-digest
node third_party/repo-tooling/check-vendored-dep.mjs --require-upstream
```

Then update the exact `version` of the `http-hardening` entry in the root
`Cargo.toml`, the `exclude` list if the survey file name changed, and the
lockfile.
