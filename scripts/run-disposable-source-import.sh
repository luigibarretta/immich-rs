#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo 'usage: run-disposable-source-import.sh --adapter <apple-photos|picasa> --binary <path> --commit-sha <sha> --output <path> [--oracle <path> --benchmark-output <path> --samples <n> --warmups <n>]' >&2
  exit 2
}

ADAPTER=''
BINARY=''
COMMIT_SHA=''
OUTPUT=''
ORACLE=''
BENCHMARK_OUTPUT=''
SAMPLES=6
WARMUPS=2
while (($# > 0)); do
  case "$1" in
    --adapter) (($# >= 2)) || usage; ADAPTER=$2; shift 2 ;;
    --binary) (($# >= 2)) || usage; BINARY=$2; shift 2 ;;
    --commit-sha) (($# >= 2)) || usage; COMMIT_SHA=$2; shift 2 ;;
    --output) (($# >= 2)) || usage; OUTPUT=$2; shift 2 ;;
    --oracle) (($# >= 2)) || usage; ORACLE=$2; shift 2 ;;
    --benchmark-output) (($# >= 2)) || usage; BENCHMARK_OUTPUT=$2; shift 2 ;;
    --samples) (($# >= 2)) || usage; SAMPLES=$2; shift 2 ;;
    --warmups) (($# >= 2)) || usage; WARMUPS=$2; shift 2 ;;
    *) usage ;;
  esac
done
[[ "$ADAPTER" == apple-photos || "$ADAPTER" == picasa ]] || usage
[[ -f "$BINARY" && -x "$BINARY" && "$COMMIT_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
BINARY=$(realpath -- "$BINARY")
OUTPUT=$(realpath -m -- "$OUTPUT")
case "$OUTPUT" in "$ROOT"/.artifacts/*) ;; *) usage ;; esac
[[ ! -e "$OUTPUT" ]] || { echo 'evidence output already exists' >&2; exit 2; }
mkdir -p -- "$ROOT/.artifacts"
if [[ -n "$ORACLE$BENCHMARK_OUTPUT" ]]; then
  [[ -f "$ORACLE" && -x "$ORACLE" && -n "$BENCHMARK_OUTPUT" ]] || usage
  [[ "$SAMPLES" =~ ^[0-9]+$ && "$WARMUPS" =~ ^[0-9]+$ ]] || usage
  ((SAMPLES >= 2 && SAMPLES <= 10 && WARMUPS <= 2)) || usage
  ORACLE=$(realpath -- "$ORACLE")
  BENCHMARK_OUTPUT=$(realpath -m -- "$BENCHMARK_OUTPUT")
  case "$BENCHMARK_OUTPUT" in "$ROOT"/.artifacts/*) ;; *) usage ;; esac
  [[ ! -e "$BENCHMARK_OUTPUT" ]] || { echo 'benchmark output already exists' >&2; exit 2; }
fi

SUFFIX="$(date -u +%Y%m%dT%H%M%SZ)-$$-$(openssl rand -hex 4)"
RUN_ID="source-import-$ADAPTER-$SUFFIX"
PROJECT="immichrs-${ADAPTER//-/}-$SUFFIX"
PROJECT=${PROJECT,,}
COMPOSE="$ROOT/tests/disposable/compose.import.yml"
WORKSPACE=$(mktemp -d "/tmp/immich-rs-source-import.XXXXXX")
DISPOSABLE_DB_PASSWORD="synthetic-db-$SUFFIX"
DISPOSABLE_HOST_PORT=$(python3 -c \
  'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
export DISPOSABLE_DB_PASSWORD DISPOSABLE_HOST_PORT DISPOSABLE_RUN_ID="$RUN_ID"
IMAGES=$(docker compose --project-name "$PROJECT" --file "$COMPOSE" config --format json | \
  jq -c '{server:.services.server.image,valkey:.services.redis.image,database:.services.database.image}')
CLEANED=0
DRIVER_CONTAINER=''
FORWARD_PID=''
NETWORK="${PROJECT}_default"
if [[ -n "${HOSTNAME:-}" ]]; then
  candidate=$(docker inspect --format '{{.Id}}' "$HOSTNAME" 2>/dev/null || true)
  if [[ -n "$candidate" && "$candidate" == "$HOSTNAME"* ]]; then
    DRIVER_CONTAINER=$candidate
  fi
fi

cleanup() {
  set +e
  if [[ -n "$FORWARD_PID" ]]; then
    kill "$FORWARD_PID" >/dev/null 2>&1
    wait "$FORWARD_PID" >/dev/null 2>&1
  fi
  if [[ -n "$DRIVER_CONTAINER" ]]; then
    docker network disconnect --force "$NETWORK" "$DRIVER_CONTAINER" >/dev/null 2>&1
  fi
  docker compose --project-name "$PROJECT" --file "$COMPOSE" down \
    --volumes --remove-orphans >/dev/null 2>&1
  case "$WORKSPACE" in
    /tmp/immich-rs-source-import.*) find "$WORKSPACE" -depth -delete ;;
  esac
  unset DISPOSABLE_DB_PASSWORD DISPOSABLE_HOST_PORT DISPOSABLE_RUN_ID
  set -e
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

echo "starting disposable $ADAPTER verification" >&2
docker compose --project-name "$PROJECT" --file "$COMPOSE" up --detach --pull missing
SERVER_CONTAINER=$(docker compose --project-name "$PROJECT" --file "$COMPOSE" ps --quiet server)
read -r HOST_IP HOST_PORT < <(docker inspect --format \
  '{{with (index .HostConfig.PortBindings "2283/tcp")}}{{(index . 0).HostIp}} {{(index . 0).HostPort}}{{end}}' \
  "$SERVER_CONTAINER")
[[ "$HOST_IP" == 127.0.0.1 && "$HOST_PORT" =~ ^[0-9]+$ ]] || {
  echo "disposable server port escaped loopback: host=$HOST_IP port=$HOST_PORT" >&2
  exit 1
}
[[ "$HOST_PORT" == "$DISPOSABLE_HOST_PORT" ]] || {
  echo 'disposable server did not bind its reserved loopback port' >&2
  exit 1
}
if [[ -n "$DRIVER_CONTAINER" ]]; then
  [[ $(docker network inspect --format \
    '{{index .Labels "com.docker.compose.project"}}' "$NETWORK") == "$PROJECT" ]]
  docker network connect "$NETWORK" "$DRIVER_CONTAINER"
  SERVER_NAME=$(docker inspect --format '{{.Name}}' "$SERVER_CONTAINER" | sed 's#^/##')
  python3 "$ROOT/scripts/loopback-forward.py" --listen-port "$HOST_PORT" \
    --target-container "$SERVER_NAME" &
  FORWARD_PID=$!
fi
ENDPOINT="http://127.0.0.1:$HOST_PORT"
READY=0
for _attempt in $(seq 1 120); do
  if curl --fail --silent --max-time 2 "$ENDPOINT/api/server/ping" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done
[[ "$READY" == 1 ]] || { echo 'disposable Immich did not become ready' >&2; exit 1; }

echo 'creating synthetic disposable account' >&2
EMAIL="synthetic-$ADAPTER-$SUFFIX@example.invalid"
PASSWORD="synthetic-admin-$SUFFIX"
SIGNUP=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email:$email,password:$password,name:"Synthetic Import Admin"}')
curl --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$SIGNUP" "$ENDPOINT/api/auth/admin-sign-up" >/dev/null
LOGIN_REQUEST=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email:$email,password:$password}')
LOGIN=$(curl --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$LOGIN_REQUEST" "$ENDPOINT/api/auth/login")
ACCESS_TOKEN=$(jq -er '.accessToken | select(type == "string" and length > 0)' <<<"$LOGIN")
PERMISSIONS='["asset.upload","asset.read","asset.update","album.read","album.create","albumAsset.create","server.about","user.read"]'
KEY_REQUEST=$(jq -nc --argjson permissions "$PERMISSIONS" \
  '{name:"immich-rs disposable source import",permissions:$permissions}')
KEY_RESPONSE=$(curl --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST --data "$KEY_REQUEST" \
  "$ENDPOINT/api/api-keys")
API_KEY=$(jq -er '.secret | select(type == "string" and length > 0)' <<<"$KEY_RESPONSE")

if [[ "$ADAPTER" == apple-photos ]]; then
  MANIFEST="$ROOT/tests/fixtures/v3/synthetic-apple-photos/manifest.json"
  ARCHIVE_VIEW='icloud-split'
  SOURCE_OPTIONS=(--album-mode folder)
  EXPECTED_ASSETS=5
  EXPECTED_BYTES=342
  EXPECTED_METADATA=0
  EXPECTED_MUTATIONS=7
else
  MANIFEST="$ROOT/tests/fixtures/v4/synthetic-picasa/manifest.json"
  ARCHIVE_VIEW='picasa-split'
  SOURCE_OPTIONS=(--album-mode none --picasa-albums --filename-date)
  EXPECTED_ASSETS=4
  EXPECTED_BYTES=262
  EXPECTED_METADATA=2
  EXPECTED_MUTATIONS=8
fi
echo 'materializing synthetic source views' >&2
DIRECTORY="$WORKSPACE/directory"
ARCHIVES="$WORKSPACE/archives"
python3 "$ROOT/scripts/materialize-fixture.py" "$MANIFEST" "$DIRECTORY"
python3 "$ROOT/scripts/materialize-fixture.py" "$MANIFEST" "$ARCHIVES" --archive-view "$ARCHIVE_VIEW"
mapfile -d '' ARCHIVE_FILES < <(find "$ARCHIVES" -maxdepth 1 -type f -name '*.zip' -print0 | sort -z)
[[ ${#ARCHIVE_FILES[@]} -eq 2 ]]
DIRECTORY_PLAN="$WORKSPACE/directory-plan.json"
ARCHIVE_PLAN="$WORKSPACE/archive-plan.json"
PLAN_COMMAND=(plan upload "$ADAPTER" --server "$ENDPOINT" "${SOURCE_OPTIONS[@]}")
echo 'planning directory source' >&2
if ! IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${PLAN_COMMAND[@]}" "$DIRECTORY" \
  >"$DIRECTORY_PLAN" 2>"$WORKSPACE/directory-plan.stderr"; then
  sed -n '1,20p' "$WORKSPACE/directory-plan.stderr" >&2
  exit 1
fi
echo 'planning split archive source' >&2
if ! IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${PLAN_COMMAND[@]}" "${ARCHIVE_FILES[@]}" \
  >"$ARCHIVE_PLAN" 2>"$WORKSPACE/archive-plan.stderr"; then
  sed -n '1,20p' "$WORKSPACE/archive-plan.stderr" >&2
  exit 1
fi
PLAN_NORMALIZER='del(.operations[].created_at_unix_ms, .operations[].modified_at_unix_ms)'
jq --sort-keys "$PLAN_NORMALIZER" "$DIRECTORY_PLAN" >"$WORKSPACE/directory-semantic.json"
jq --sort-keys "$PLAN_NORMALIZER" "$ARCHIVE_PLAN" >"$WORKSPACE/archive-semantic.json"
cmp --silent "$WORKSPACE/directory-semantic.json" "$WORKSPACE/archive-semantic.json"
SUMMARY=$(jq -c '.summary' "$DIRECTORY_PLAN")
echo 'validating deterministic plans' >&2
[[ $(jq -r '.operations' <<<"$SUMMARY") == "$EXPECTED_ASSETS" ]]
[[ $(jq -r '.media_bytes' <<<"$SUMMARY") == "$EXPECTED_BYTES" ]]
[[ $(jq -r '.metadata_updates // 0' <<<"$SUMMARY") == "$EXPECTED_METADATA" ]]
[[ $(jq -r '.album_creates' <<<"$SUMMARY") == 1 ]]
[[ $(jq -r '.album_memberships' <<<"$SUMMARY") == 1 ]]
[[ $(jq -r '.max_mutations' <<<"$SUMMARY") == "$EXPECTED_MUTATIONS" ]]

CHECKPOINT="$WORKSPACE/directory.sqlite"
ARCHIVE_CHECKPOINT="$WORKSPACE/archive.sqlite"
DRY_REPORT=$(env -u IMMICH_RS_API_KEY "$BINARY" apply upload --dry-run \
  --plan "$DIRECTORY_PLAN" --source "$DIRECTORY" --checkpoint "$CHECKPOINT" \
  "${SOURCE_OPTIONS[@]}")
[[ ! -e "$CHECKPOINT" && $(jq -r '.would_upload' <<<"$DRY_REPORT") == "$EXPECTED_ASSETS" ]]
APPLY_BASE=(apply upload --server "$ENDPOINT" --plan "$DIRECTORY_PLAN" \
  --source "$DIRECTORY" --checkpoint "$CHECKPOINT" "${SOURCE_OPTIONS[@]}")
FIRST_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${APPLY_BASE[@]}")
echo 'validating idempotent apply and archive convergence' >&2
RESUME_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${APPLY_BASE[@]}")
ARCHIVE_APPLY=(apply upload --server "$ENDPOINT" --plan "$ARCHIVE_PLAN" \
  --checkpoint "$ARCHIVE_CHECKPOINT" "${SOURCE_OPTIONS[@]}")
for path in "${ARCHIVE_FILES[@]}"; do ARCHIVE_APPLY+=(--input "$path"); done
ARCHIVE_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${ARCHIVE_APPLY[@]}")
[[ $(jq -r '.created' <<<"$FIRST_REPORT") == "$EXPECTED_ASSETS" ]]
[[ $(jq -r '.metadata_updated' <<<"$FIRST_REPORT") == "$EXPECTED_METADATA" ]]
[[ $(jq -r '.resumed_effects' <<<"$RESUME_REPORT") == "$EXPECTED_MUTATIONS" ]]
[[ $(jq -r '.duplicate' <<<"$ARCHIVE_REPORT") == "$EXPECTED_ASSETS" ]]
[[ -z $(find "$WORKSPACE" -maxdepth 1 -name '.immich-rs-stage-*' -print -quit) ]]
if grep -aF -- "$API_KEY" "$DIRECTORY_PLAN" "$ARCHIVE_PLAN" "$CHECKPOINT" "$ARCHIVE_CHECKPOINT" >/dev/null; then
  echo 'API key entered a durable artifact' >&2
  exit 1
fi

echo 'validating observable server state' >&2
SEARCH_REQUEST='{"size":100,"withExif":true}'
curl --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST --data "$SEARCH_REQUEST" \
  "$ENDPOINT/api/search/metadata" >"$WORKSPACE/search.json"
curl --fail --silent --get --header "Authorization: Bearer $ACCESS_TOKEN" \
  --data 'isOwned=true' "$ENDPOINT/api/albums" >"$WORKSPACE/albums.json"
OBSERVED_ASSETS=$(jq -er '.assets.items | length' "$WORKSPACE/search.json")
OBSERVED_ALBUMS=$(jq -er 'length' "$WORKSPACE/albums.json")
[[ "$OBSERVED_ASSETS" == "$EXPECTED_ASSETS" && "$OBSERVED_ALBUMS" == 1 ]]
ALBUM_ID=$(jq -er '.[0].id' "$WORKSPACE/albums.json")
ALBUM_SEARCH=$(jq -nc --arg id "$ALBUM_ID" '{size:100,albumIds:[$id]}')
curl --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST --data "$ALBUM_SEARCH" \
  "$ENDPOINT/api/search/metadata" >"$WORKSPACE/album-assets.json"
OBSERVED_MEMBERS=$(jq -er '.assets.items | length' "$WORKSPACE/album-assets.json")
LIVE_LINKS=$(jq -r '[.assets.items[] | select(.livePhotoVideoId != null)] | length' "$WORKSPACE/search.json")
DESCRIPTIONS=$(jq -r '[.assets.items[] | select(.exifInfo.description? != null and .exifInfo.description != "")] | length' "$WORKSPACE/search.json")
[[ "$OBSERVED_MEMBERS" == "$EXPECTED_ASSETS" && "$LIVE_LINKS" == 1 ]]
if [[ "$ADAPTER" == picasa ]]; then [[ "$DESCRIPTIONS" == 1 ]]; fi

if [[ -n "$BENCHMARK_OUTPUT" ]]; then
  echo 'running paired compatibility-intersection benchmark' >&2
  BENCHMARK_MANIFEST="$ROOT/benchmarks/fixtures/source-import-64m.json"
  BENCHMARK_SOURCE="$WORKSPACE/benchmark-source"
  BENCHMARK_WORKSPACE="$WORKSPACE/benchmark-work"
  python3 "$ROOT/scripts/materialize-fixture.py" "$BENCHMARK_MANIFEST" "$BENCHMARK_SOURCE"
  mkdir -- "$BENCHMARK_WORKSPACE"
  IMMICH_RS_BENCHMARK_ADMIN_TOKEN="$ACCESS_TOKEN" \
    python3 "$ROOT/scripts/benchmark-source-import.py" --adapter "$ADAPTER" \
      --endpoint "$ENDPOINT" --run-id "$SUFFIX" --source-revision "$COMMIT_SHA" \
      --source "$BENCHMARK_SOURCE" --fixture-manifest "$BENCHMARK_MANIFEST" \
      --workspace "$BENCHMARK_WORKSPACE" --immich-rs "$BINARY" --oracle "$ORACLE" \
      --samples "$SAMPLES" --warmups "$WARMUPS" --output "$BENCHMARK_OUTPUT"
fi

SERVER_VERSION=$(curl --fail --silent "$ENDPOINT/api/server/version")
BINARY_SHA256=$(sha256sum "$BINARY" | cut -d' ' -f1)
MANIFEST_SHA256=$(sha256sum "$MANIFEST" | cut -d' ' -f1)
OPERATING_SYSTEM=$(uname -s)
ARCHITECTURE=$(uname -m)
DOCKER_VERSION=$(docker version --format '{{.Server.Version}}')
COMPOSE_VERSION=$(docker compose version --short)
cleanup
CLEANED=1
trap - EXIT HUP INT TERM
REMAINING_CONTAINERS=$(docker ps --all --quiet --filter "label=com.docker.compose.project=$PROJECT")
REMAINING_VOLUMES=$(docker volume ls --quiet --filter "label=com.docker.compose.project=$PROJECT")
REMAINING_NETWORKS=$(docker network ls --quiet --filter "label=com.docker.compose.project=$PROJECT")
[[ -z "$REMAINING_CONTAINERS$REMAINING_VOLUMES$REMAINING_NETWORKS" ]]

jq -n --arg adapter "$ADAPTER" --arg commit "$COMMIT_SHA" \
  --arg binary_sha256 "$BINARY_SHA256" --arg manifest_sha256 "$MANIFEST_SHA256" \
  --arg operating_system "$OPERATING_SYSTEM" --arg architecture "$ARCHITECTURE" \
  --arg docker_version "$DOCKER_VERSION" --arg compose_version "$COMPOSE_VERSION" \
  --argjson images "$IMAGES" \
  --argjson version "$SERVER_VERSION" --argjson summary "$SUMMARY" \
  --argjson dry "$DRY_REPORT" --argjson first "$FIRST_REPORT" \
  --argjson resume "$RESUME_REPORT" --argjson duplicate "$ARCHIVE_REPORT" \
  --argjson assets "$OBSERVED_ASSETS" --argjson albums "$OBSERVED_ALBUMS" \
  --argjson members "$OBSERVED_MEMBERS" --argjson live "$LIVE_LINKS" \
  --argjson descriptions "$DESCRIPTIONS" --argjson cleaned "$CLEANED" \
  '{schema:"source-import-disposable-v1",adapter:$adapter,commit_sha:$commit,
    binary_sha256:$binary_sha256,fixture:{kind:"synthetic",license:"CC0-1.0",manifest_sha256:$manifest_sha256},
    environment:{os:$operating_system,architecture:$architecture,docker:$docker_version,compose:$compose_version},
    images:$images,server_version:$version,
    isolation:{published_loopback:true,production_endpoint:false,production_credentials:false},
    plan:{directory_zip_semantically_equivalent:true,transport_timestamps_excluded:true,summary:$summary},
    reports:{dry_run:$dry,first:$first,resume:$resume,fresh_checkpoint_duplicate:$duplicate},
    postconditions:{assets:$assets,owned_albums:$albums,album_members:$members,live_photo_links:$live,descriptions:$descriptions},
    cleanup:{verified:($cleaned == 1),labelled_containers:0,labelled_volumes:0,labelled_networks:0,staging_files:0,credentials_removed:true}}' >"$OUTPUT"
