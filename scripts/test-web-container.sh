#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
REVISION=""
VERSION="0.0.0"

usage() {
    echo "usage: test-web-container.sh --revision SHA [--version VERSION]" >&2
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --revision) REVISION=${2:-}; shift 2 ;;
        --version) VERSION=${2:-}; shift 2 ;;
        *) usage; exit 2 ;;
    esac
done

if [[ ! "$REVISION" =~ ^[0-9a-f]{40}$ ]] ||
    [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[1-9][0-9]*)?$ ]]; then
    usage
    exit 2
fi
for command in cmp docker mktemp; do
    command -v "$command" >/dev/null || {
        echo "required command is missing: $command" >&2
        exit 2
    }
done

IMAGE="immich-rs-web-container-test:${REVISION}"
CONTAINER="immich-rs-web-container-test-${REVISION}"
if docker image inspect "$IMAGE" >/dev/null 2>&1 ||
    docker container inspect "$CONTAINER" >/dev/null 2>&1; then
    echo "refusing to replace pre-existing web container test resources" >&2
    exit 2
fi
ARTIFACT_ROOT="$ROOT/.artifacts"
mkdir -p "$ARTIFACT_ROOT"
TEST_ROOT=$(mktemp -d "$ARTIFACT_ROOT/web-container.XXXXXX")

cleanup() {
    docker container rm --force "$CONTAINER" >/dev/null 2>&1 || true
    docker image rm "$IMAGE" >/dev/null 2>&1 || true
    rm -rf -- "$TEST_ROOT"
}
trap cleanup EXIT

docker buildx build --load --platform linux/amd64 \
    --build-arg "VERSION=$VERSION" \
    --build-arg "VCS_REF=$REVISION" \
    --tag "$IMAGE" --file "$ROOT/Containerfile.web" "$ROOT"

test "$(docker image inspect "$IMAGE" --format '{{.Config.User}}')" = "65532:65532"
test "$(docker image inspect "$IMAGE" \
    --format '{{index .Config.Labels "org.opencontainers.image.title"}}')" = "immich-rs-web"
test "$(docker image inspect "$IMAGE" \
    --format '{{index .Config.Labels "org.opencontainers.image.revision"}}')" = "$REVISION"

for output in first second; do
    docker run --name "$CONTAINER" --network none --read-only --user 65532:65532 \
        --cap-drop ALL --security-opt no-new-privileges --pids-limit 128 \
        --tmpfs /tmp:rw,noexec,nosuid,nodev,size=16m \
        "$IMAGE" --help > "$TEST_ROOT/$output.txt"
    docker container rm "$CONTAINER" >/dev/null
done
cmp "$TEST_ROOT/first.txt" "$TEST_ROOT/second.txt"
grep -F "Usage: immich-rs-web [--config PATH]" "$TEST_ROOT/first.txt" >/dev/null
