#!/usr/bin/env bash
# Runs the normalized read-only black-box differential against the pinned
# immich-go oracle. Expects scripts/fetch-oracle.py to have verified the oracle
# under .cache/oracle and target/debug/immich-rs to be built from this tree
# (the oracle-fetch and differential-build steps of ci-local.json).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

oracle=.cache/oracle/immich-go-0.32.0-linux-x86_64/immich-go
subject=target/debug/immich-rs
output=.artifacts/oracle
mkdir -p "${output}"

python3 scripts/run-oracle.py \
  tests/oracle/cases/folder-matrix-v1.json \
  --oracle "${oracle}" \
  --output "${output}/observation.json"
python3 scripts/compare-oracle.py \
  tests/oracle/compatibility/folder-matrix-v1.json \
  "${output}/observation.json" \
  --output "${output}/differential-report.json"
python3 scripts/run-oracle.py \
  tests/oracle/cases/google-takeout-basic-v1.json \
  --oracle "${oracle}" \
  --output "${output}/google-takeout-observation.json"
python3 scripts/compare-takeout-oracle.py \
  tests/oracle/compatibility/google-takeout-basic-v1.json \
  "${output}/google-takeout-observation.json" \
  --output "${output}/google-takeout-report.json"
python3 scripts/run-oracle.py \
  tests/oracle/cases/google-takeout-complete-v2.json \
  --oracle "${oracle}" \
  --output "${output}/google-takeout-complete-observation.json"
python3 scripts/compare-takeout-complete.py \
  tests/oracle/compatibility/google-takeout-complete-v2.json \
  "${output}/google-takeout-complete-observation.json" \
  --output "${output}/google-takeout-complete-report.json"
python3 scripts/run-oracle.py \
  tests/oracle/cases/apple-photos-v3.json \
  --oracle "${oracle}" \
  --output "${output}/apple-photos-observation.json"
python3 scripts/compare-apple-oracle.py \
  tests/oracle/compatibility/apple-photos-v3.json \
  "${output}/apple-photos-observation.json" \
  --output "${output}/apple-photos-report.json"
python3 scripts/run-oracle.py \
  tests/oracle/cases/picasa-directory-v4.json \
  --oracle "${oracle}" \
  --output "${output}/picasa-directory-observation.json"
python3 scripts/run-oracle.py \
  tests/oracle/cases/picasa-v4.json \
  --oracle "${oracle}" \
  --output "${output}/picasa-zip-observation.json"
python3 scripts/compare-picasa-oracle.py \
  tests/oracle/compatibility/picasa-v4.json \
  "${output}/picasa-directory-observation.json" \
  "${output}/picasa-zip-observation.json" \
  --output "${output}/picasa-report.json"
python3 scripts/run-import-differential.py \
  tests/oracle/compatibility/apple-import-v1.json \
  --oracle "${oracle}" \
  --immich-rs "${subject}" \
  --output "${output}/apple-import-report.json"
python3 scripts/run-import-differential.py \
  tests/oracle/compatibility/picasa-import-v1.json \
  --oracle "${oracle}" \
  --immich-rs "${subject}" \
  --output "${output}/picasa-import-report.json"
python3 scripts/run-migration-differential.py \
  tests/oracle/compatibility/immich-migration-v1.json \
  --oracle "${oracle}" \
  --immich-rs "${subject}" \
  --output "${output}/immich-migration-report.json"
