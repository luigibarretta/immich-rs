# auth-modules provenance

The `auth-modules` source is vendored so the CLI, the Web Console, both
container images and the public mirror build from one checkout without
cloning a private repository. It is an exact `git archive` of
`lib-auth-modules` tag `v0.2.0`, the revision recorded in
`dependencies/auth-modules.sha` (`2a9aec766b0edb4cf0a55f8a4acd246b73b8a078`).
The crate is licensed `MIT OR Apache-2.0`, compatible with AGPL distribution
(ADR-0003). immich-rs enables only the `secret-files` feature, which adds no
transitive dependency (ADR-0035).

`dependencies/vendored.json` declares this copy. The `vendored-deps` step
(`check-vendored-dep`) verifies the recorded tree digest in
`dependencies/auth-modules-vendor.sha256` and, when a clone of
`lib-auth-modules` is available (`.ci/auth-modules` or `../lib-auth-modules`),
compares every file with the pinned commit.

To refresh:

```bash
rm -rf vendor/auth-modules && mkdir vendor/auth-modules
git -C ../lib-auth-modules archive <commit> | tar -x -C vendor/auth-modules
git -C ../lib-auth-modules rev-parse <commit> > dependencies/auth-modules.sha
node third_party/repo-tooling/check-vendored-dep.mjs --only auth-modules --update-digest
node third_party/repo-tooling/check-vendored-dep.mjs --require-upstream
```

Then update the exact `version` of the `auth-modules` entry in the root
`Cargo.toml` and the lockfile.
