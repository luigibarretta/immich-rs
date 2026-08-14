#!/usr/bin/env sh
set -eu

oracle=${IMMICH_GO_ORACLE:-/usr/local/bin/immich-go}
expected_version=0.32.0
expected_sha256=eb46733ccf8ff78fb7207b9601695ff5730927cc712f08c6a93e315c845d5475

if [ ! -x "$oracle" ]; then
  echo "immich-go oracle is not executable: $oracle" >&2
  exit 1
fi

reported_version=$($oracle --version 2>&1)
case "$reported_version" in
  *" $expected_version") ;;
  *)
    echo "unexpected immich-go version: $reported_version" >&2
    exit 1
    ;;
esac

actual_sha256=$(sha256sum "$oracle" | awk '{print $1}')
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "immich-go oracle digest mismatch" >&2
  exit 1
fi

printf 'verified immich-go oracle %s (%s)\n' "$expected_version" "$actual_sha256"
