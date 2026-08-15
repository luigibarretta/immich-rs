#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo 'usage: prepare-phase2-benchmark-corpus.sh --source <path> --source-manifest <path> --output <path> --output-manifest <path>' >&2
  exit 2
}

SOURCE=''
SOURCE_MANIFEST=''
OUTPUT=''
OUTPUT_MANIFEST=''
while (($# > 0)); do
  case "$1" in
    --source) (($# >= 2)) || usage; SOURCE=$2; shift 2 ;;
    --source-manifest) (($# >= 2)) || usage; SOURCE_MANIFEST=$2; shift 2 ;;
    --output) (($# >= 2)) || usage; OUTPUT=$2; shift 2 ;;
    --output-manifest) (($# >= 2)) || usage; OUTPUT_MANIFEST=$2; shift 2 ;;
    *) usage ;;
  esac
done

[[ -d "$SOURCE" && -f "$SOURCE_MANIFEST" && -n "$OUTPUT" && -n "$OUTPUT_MANIFEST" ]] || usage
SOURCE=$(realpath -- "$SOURCE")
SOURCE_MANIFEST=$(realpath -- "$SOURCE_MANIFEST")
OUTPUT=$(realpath -m -- "$OUTPUT")
OUTPUT_MANIFEST=$(realpath -m -- "$OUTPUT_MANIFEST")
case "$SOURCE:$SOURCE_MANIFEST:$OUTPUT:$OUTPUT_MANIFEST" in
  /tmp/immich-rs-disposable.*/*:/tmp/immich-rs-disposable.*/*:/tmp/immich-rs-disposable.*/*:/tmp/immich-rs-disposable.*/*) ;;
  *) echo 'benchmark corpus paths must remain inside one disposable workspace' >&2; exit 2 ;;
esac
[[ "$OUTPUT_MANIFEST" != "$OUTPUT"/* ]] || { echo 'output manifest must remain outside the media source' >&2; exit 2; }
jq -e '
  .schema == "phase2-corpus-v1" and
  .fixture_id == "synthetic-phase2-media-matrix" and
  .synthetic == true and
  .license == "CC0-1.0" and
  .generator.recipe == "ffmpeg-lavfi-apple-live-v2" and
  .expected == {upload_operations:4,xmp_sidecars:1,live_photo_pairs:1}
' "$SOURCE_MANIFEST" >/dev/null || { echo 'source corpus contract drifted' >&2; exit 1; }

while IFS=$'\t' read -r path expected_digest; do
  [[ "$path" =~ ^[a-z0-9.-]+$ && -f "$SOURCE/$path" ]] || { echo 'source corpus path is invalid' >&2; exit 1; }
  [[ $(sha256sum "$SOURCE/$path" | cut -d' ' -f1) == "$expected_digest" ]] || {
    echo 'source corpus digest drifted' >&2
    exit 1
  }
done < <(jq -r '.files[] | [.path,.sha256] | @tsv' "$SOURCE_MANIFEST")

declare -a EXPECTED_FILES=(clip.mp4 image.jpg image.xmp motion.mov still.jpg)
mkdir -p -- "$OUTPUT"
if find "$OUTPUT" -mindepth 1 -maxdepth 1 ! -type f -print -quit | grep -q .; then
  echo 'benchmark corpus contains an undeclared non-file entry' >&2
  exit 1
fi
while IFS= read -r existing; do
  relative=${existing#"$OUTPUT"/}
  if [[ ! " ${EXPECTED_FILES[*]} " =~ " $relative " ]]; then
    echo 'benchmark corpus contains an undeclared file' >&2
    exit 1
  fi
done < <(find "$OUTPUT" -mindepth 1 -maxdepth 1 -type f -print | sort)

cp --reflink=auto --sparse=always -- "$SOURCE/clip.mp4" "$OUTPUT/clip.mp4"
cp --reflink=auto --sparse=always -- "$SOURCE/image.jpg" "$OUTPUT/image.jpg"
cp --reflink=auto --sparse=always -- "$SOURCE/image.xmp" "$OUTPUT/image.xmp"
cp --reflink=auto --sparse=always -- "$SOURCE/live.mov" "$OUTPUT/motion.mov"
cp --reflink=auto --sparse=always -- "$SOURCE/live.jpg" "$OUTPUT/still.jpg"

files='[]'
for entry in \
  'clip.mp4:standalone-video' \
  'image.jpg:standalone-image' \
  'image.xmp:xmp-sidecar' \
  'motion.mov:standalone-video' \
  'still.jpg:standalone-image'; do
  path=${entry%%:*}
  role=${entry#*:}
  bytes=$(stat --format '%s' "$OUTPUT/$path")
  digest=$(sha256sum "$OUTPUT/$path" | cut -d' ' -f1)
  files=$(jq -c --arg path "$path" --arg role "$role" --arg sha "$digest" \
    --argjson bytes "$bytes" '. + [{path:$path,role:$role,bytes:$bytes,sha256:$sha}]' <<<"$files")
done
source_digest=$(sha256sum "$SOURCE_MANIFEST" | cut -d' ' -f1)
jq -n --arg source_digest "$source_digest" --argjson files "$files" \
  '{schema:"phase2-benchmark-corpus-v1",fixture_id:"synthetic-phase2-standalone-matrix",synthetic:true,license:"CC0-1.0",derived_from:{schema:"phase2-corpus-v1",manifest_sha256:$source_digest,mapping:"live bytes renamed to distinct standalone basenames"},files:$files,expected:{upload_operations:4,xmp_sidecars:1,live_photo_pairs:0,visible_assets:4,live_photo_links:0}}' \
  >"$OUTPUT_MANIFEST"
