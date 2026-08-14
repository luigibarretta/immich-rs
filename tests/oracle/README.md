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

`scripts/run-oracle.py` invokes the oracle as a subprocess over a declared v1
synthetic fixture and the bounded local mock server. It verifies the baseline
version and executable digest first, captures stdout, stderr, exit status and
server-side requests, then emits `oracle-observation-v1` with volatile values
normalized. A mutating request fails unless the case declares the exact method,
path, JSON body and `oracle_defect` classification. The folder case records a
v0.32.0 defect: immich-go sends five job-resume `PUT` requests even with
`--dry-run --pause-immich-jobs=false`. The loopback mock contains those calls;
no asset or metadata mutation is accepted. Any drift or additional mutation
fails closed.

`scripts/fetch-oracle.py` downloads the official release only on a cache miss,
verifies the pinned archive and binary SHA-256 values, and extracts only the
bounded regular `immich-go` member. The cache and executable are never included
in repository or CI evidence artifacts.

`scripts/compare-oracle.py` projects the normalized observation onto the
declared Phase 1 compatibility matrix and compares it with the versioned golden
plan. It never imports Go packages, copies source, bundles the executable in
releases or accepts a production server target.

`tests/oracle/mock_immich_server.py` records bounded requests and supports
authentication rejection, incompatible versions, timeout, 429, 5xx,
disconnect and committed-mutation-with-lost-response scenarios. It binds only
to loopback and uses the fixed `synthetic-oracle-key` test value.

The daily homelab `verify-immich-go.yml` job is a separate production
compatibility smoke: it uses a least-privilege key, dry-run and an empty future
date range. Do not expand that scheduled job into a mutating differential test.
