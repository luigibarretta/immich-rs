#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BINFMT_IMAGE="docker.io/tonistiigi/binfmt@sha256:400a4873b838d1b89194d982c45e5fb3cda4593fbfd7e08a02e76b03b21166f0"
SBOM_SCANNER="docker.io/docker/buildkit-syft-scanner@sha256:ae4f3b554449e7e25548e7d8ccc029d17357348e30c6e3df01b92bc93654d6a9"
BUILDKIT_IMAGE="docker.io/moby/buildkit@sha256:28a898719c18a33f4e8000685287fa36fd0dd9560c6440227d3a732d79bb41d8"
BUILDER=""
OUTPUT=""
REPORT=""
REVISION=""
VERSION="0.0.0"
PRODUCT=""
CONTAINERFILE=""
INSTALLED_BINFMT=0

usage() {
    echo "usage: build-multiarch-container.sh --product immich-rs|immich-rs-web --output FILE --report FILE --revision SHA [--version VERSION]" >&2
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --output) OUTPUT=${2:-}; shift 2 ;;
        --report) REPORT=${2:-}; shift 2 ;;
        --revision) REVISION=${2:-}; shift 2 ;;
        --version) VERSION=${2:-}; shift 2 ;;
        --product) PRODUCT=${2:-}; shift 2 ;;
        *) usage; exit 2 ;;
    esac
done

if [[ -z "$OUTPUT" || -z "$REPORT" || ! "$REVISION" =~ ^[0-9a-f]{40}$ ]]; then
    usage
    exit 2
fi
case "$PRODUCT" in
    immich-rs) CONTAINERFILE="$ROOT/Containerfile" ;;
    immich-rs-web) CONTAINERFILE="$ROOT/Containerfile.web" ;;
    *) usage; exit 2 ;;
esac
BUILDER="${PRODUCT}-multiarch-$$"
if [[ -e "$OUTPUT" || -L "$OUTPUT" || -e "$REPORT" || -L "$REPORT" ]]; then
    echo "container output or report already exists" >&2
    exit 2
fi
for command in docker git python3; do
    command -v "$command" >/dev/null || {
        echo "required command is missing: $command" >&2
        exit 2
    }
done
SOURCE_REVISION=$(git -C "$ROOT" rev-parse HEAD)
if [[ "$SOURCE_REVISION" != "$REVISION" ]]; then
    echo "container revision does not match the source checkout" >&2
    exit 2
fi
if [[ -n "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)" ]]; then
    echo "container source checkout must be clean" >&2
    exit 2
fi

cleanup() {
    docker buildx rm "$BUILDER" >/dev/null 2>&1 || true
    if [[ "$INSTALLED_BINFMT" -eq 1 ]]; then
        docker run --privileged --rm "$BINFMT_IMAGE" --uninstall qemu-aarch64 >/dev/null
    fi
}
trap cleanup EXIT

mkdir -p "$(dirname "$OUTPUT")" "$(dirname "$REPORT")"
if [[ ! -e /proc/sys/fs/binfmt_misc/qemu-aarch64 ]]; then
    docker run --privileged --rm "$BINFMT_IMAGE" --install arm64 >/dev/null
    INSTALLED_BINFMT=1
fi
docker buildx create --name "$BUILDER" --driver docker-container \
    --driver-opt "image=$BUILDKIT_IMAGE" >/dev/null
docker buildx inspect "$BUILDER" --bootstrap >/dev/null

docker buildx build \
    --builder "$BUILDER" \
    --platform linux/amd64,linux/arm64 \
    --build-arg "VERSION=$VERSION" \
    --build-arg "VCS_REF=$REVISION" \
    --provenance=mode=max \
    --attest "type=sbom,generator=$SBOM_SCANNER" \
    --output "type=oci,dest=$OUTPUT" \
    --file "$CONTAINERFILE" \
    "$ROOT"

python3 "$ROOT/scripts/verify-oci-layout.py" \
    --archive "$OUTPUT" \
    --commit-sha "$REVISION" \
    --version "$VERSION" \
    --image-title "$PRODUCT" \
    --output "$REPORT"
