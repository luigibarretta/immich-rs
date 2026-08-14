#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
adr_dir="$repo_root/docs/adr"
minimum_count=15

count=$(find "$adr_dir" -maxdepth 1 -type f -name 'ADR-[0-9][0-9][0-9][0-9]-*.md' | wc -l)
if [ "$count" -lt "$minimum_count" ]; then
  echo "expected at least $minimum_count ADRs, found $count" >&2
  exit 1
fi

find "$adr_dir" -maxdepth 1 -type f -name 'ADR-[0-9][0-9][0-9][0-9]-*.md' -print |
  sort |
  while IFS= read -r adr; do
    grep -Eq '^- Status: (Accepted|Proposed|Superseded|Rejected)$' "$adr" || {
      echo "$adr: missing valid status" >&2
      exit 1
    }
    for heading in Context Decision Consequences Verification; do
      grep -q "^## $heading$" "$adr" || {
        echo "$adr: missing ## $heading" >&2
        exit 1
      }
    done
  done
