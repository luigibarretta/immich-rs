# immich-go oracle

The initial behavioral oracle is the official `immich-go` v0.32.0 release,
upstream commit `f7d19fce34acd4884ea2c02fc3025706a060afdf`.

On the homelab control node the verified executable is
`/usr/local/bin/immich-go`. Run:

```bash
scripts/verify-local-oracle.sh
```

The verifier checks both the reported version and the exact Linux x86-64
binary digest. It does not call Immich or inspect media.

Later differential tests must invoke the oracle as a subprocess over synthetic
fixtures and normalize its observable plan/result. They must not import its Go
packages, copy source, bundle the executable in releases or use the production
Immich key.

The daily homelab `verify-immich-go.yml` job is a separate production
compatibility smoke: it uses a least-privilege key, dry-run and an empty future
date range. Do not expand that scheduled job into a mutating differential test.
