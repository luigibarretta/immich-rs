#!/usr/bin/env bash
set -euo pipefail

SCRIPT_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
exec python3 "$SCRIPT_ROOT/prepare-phase2-benchmark-corpus.py" "$@"
