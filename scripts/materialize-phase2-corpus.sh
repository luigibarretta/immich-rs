#!/usr/bin/env bash
set -euo pipefail

REPOSITORY_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
LIVE_PHOTO_ID='synthetic-live-photo-v1'

usage() {
  echo 'usage: materialize-phase2-corpus.sh --image <digest-ref> --source <path> --manifest <path> --container <name> --run-label <key=value>' >&2
  exit 2
}

IMAGE=''
SOURCE=''
MANIFEST=''
CONTAINER=''
RUN_LABEL=''
while (($# > 0)); do
  case "$1" in
    --image) (($# >= 2)) || usage; IMAGE=$2; shift 2 ;;
    --source) (($# >= 2)) || usage; SOURCE=$2; shift 2 ;;
    --manifest) (($# >= 2)) || usage; MANIFEST=$2; shift 2 ;;
    --container) (($# >= 2)) || usage; CONTAINER=$2; shift 2 ;;
    --run-label) (($# >= 2)) || usage; RUN_LABEL=$2; shift 2 ;;
    *) usage ;;
  esac
done

[[ "$IMAGE" == *@sha256:* && -n "$SOURCE" && -n "$MANIFEST" ]] || usage
[[ "$CONTAINER" =~ ^immich-rs-[a-zA-Z0-9_.-]+-generator$ ]] || usage
[[ "$RUN_LABEL" =~ ^io\.immich-rs\.disposable\.run=phase2-[a-zA-Z0-9_.-]+$ ]] || usage
SOURCE=$(realpath -m -- "$SOURCE")
MANIFEST=$(realpath -m -- "$MANIFEST")
case "$SOURCE:$MANIFEST" in
  /tmp/immich-rs-disposable.*/*:/tmp/immich-rs-disposable.*/*) ;;
  *) echo 'corpus paths must remain inside a disposable workspace' >&2; exit 2 ;;
esac
[[ "$MANIFEST" != "$SOURCE"/* ]] || { echo 'manifest must be outside the media source' >&2; exit 2; }
mkdir -p -- "$SOURCE"

declare -a EXPECTED_FILES=(image.jpg image.xmp clip.mp4 live.jpg live.mov)
if find "$SOURCE" -mindepth 1 -maxdepth 1 ! -type f -print -quit | grep -q .; then
  echo 'source contains an undeclared non-file entry' >&2
  exit 1
fi
while IFS= read -r existing; do
  relative=${existing#"$SOURCE"/}
  if [[ ! " ${EXPECTED_FILES[*]} " =~ " $relative " ]]; then
    echo 'source contains an undeclared file' >&2
    exit 1
  fi
done < <(find "$SOURCE" -mindepth 1 -maxdepth 1 -type f -print | sort)

run_ffmpeg() {
  local output=$1
  shift
  docker run --rm \
    --name "$CONTAINER" \
    --label "$RUN_LABEL" \
    --network none \
    --entrypoint /usr/bin/ffmpeg \
    "$IMAGE" \
    -hide_banner -loglevel error -nostdin -y "$@" pipe:1 >"$output"
  [[ -s "$output" ]] || { echo 'synthetic media generation produced an empty file' >&2; exit 1; }
}

run_ffmpeg "$SOURCE/image.jpg" \
  -f lavfi -i 'color=c=0x123456:s=64x64:r=1' -frames:v 1 \
  -map_metadata -1 -fflags +bitexact -flags:v +bitexact -c:v mjpeg -q:v 2 -f image2pipe
printf '%s\n' \
  '<?xpacket begin="" id="synthetic-phase2-xmp"?>' \
  '<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" dc:format="image/jpeg"/></rdf:RDF></x:xmpmeta>' \
  '<?xpacket end="w"?>' >"$SOURCE/image.xmp"
run_ffmpeg "$SOURCE/clip.mp4" \
  -f lavfi -i 'testsrc2=size=640x360:rate=30:duration=2' -an -map_metadata -1 \
  -fflags +bitexact -flags:v +bitexact -threads 1 -c:v libx264 -preset medium -crf 18 \
  -pix_fmt yuv420p -movflags frag_keyframe+empty_moov+default_base_moof -f mp4
run_ffmpeg "$SOURCE/live.jpg" \
  -f lavfi -i 'color=c=0x654321:s=64x64:r=1' -frames:v 1 \
  -map_metadata -1 -fflags +bitexact -flags:v +bitexact -c:v mjpeg -q:v 2 -f image2pipe
python3 "$REPOSITORY_ROOT/scripts/add-live-photo-metadata.py" \
  --content-identifier "$LIVE_PHOTO_ID" \
  <"$SOURCE/live.jpg" >"$SOURCE/.live-with-metadata.jpg"
mv -- "$SOURCE/.live-with-metadata.jpg" "$SOURCE/live.jpg"
run_ffmpeg "$SOURCE/live.mov" \
  -f lavfi -i 'testsrc2=size=640x360:rate=30:duration=2' -vf hflip -an -map_metadata -1 \
  -fflags +bitexact -flags:v +bitexact -threads 1 -c:v libx264 -preset medium -crf 18 \
  -pix_fmt yuv420p -metadata "com.apple.quicktime.content.identifier=$LIVE_PHOTO_ID" \
  -movflags use_metadata_tags+frag_keyframe+empty_moov+default_base_moof -f mov

files='[]'
for entry in \
  'clip.mp4:standalone-video' \
  'image.jpg:standalone-image' \
  'image.xmp:xmp-sidecar' \
  'live.jpg:live-photo-image' \
  'live.mov:live-photo-video'; do
  path=${entry%%:*}
  role=${entry#*:}
  bytes=$(stat --format '%s' "$SOURCE/$path")
  digest=$(sha256sum "$SOURCE/$path" | cut -d' ' -f1)
  files=$(jq -c --arg path "$path" --arg role "$role" --arg sha "$digest" \
    --argjson bytes "$bytes" '. + [{path:$path,role:$role,bytes:$bytes,sha256:$sha}]' <<<"$files")
done

jq -n \
  --arg image "$IMAGE" \
  --argjson files "$files" \
  '{schema:"phase2-corpus-v1",fixture_id:"synthetic-phase2-media-matrix",synthetic:true,license:"CC0-1.0",generator:{network:"none",image:$image,recipe:"ffmpeg-lavfi-apple-live-v2",determinism:"pinned image, fixed filters, single-thread video encoding, stripped metadata, fixed synthetic live-photo identifier"},files:$files,expected:{upload_operations:4,xmp_sidecars:1,live_photo_pairs:1}}' \
  >"$MANIFEST"
