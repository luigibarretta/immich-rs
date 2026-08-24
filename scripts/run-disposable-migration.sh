#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo 'usage: run-disposable-migration.sh --binary <path> --commit-sha <sha> --output <path> [--oracle <path> --benchmark-output <path> --samples <n> --warmups <n>]' >&2
  exit 2
}

BINARY=''
COMMIT_SHA=''
OUTPUT=''
ORACLE=''
BENCHMARK_OUTPUT=''
SAMPLES=6
WARMUPS=2
while (($# > 0)); do
  case "$1" in
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
RUN_ID="immich-migration-$SUFFIX"
SOURCE_PROJECT="immichrs-migration-source-${SUFFIX,,}"
DESTINATION_PROJECT="immichrs-migration-destination-${SUFFIX,,}"
COMPOSE="$ROOT/tests/disposable/compose.import.yml"
WORKSPACE=$(mktemp -d "/tmp/immich-rs-migration.XXXXXX")
SOURCE_PASSWORD="synthetic-source-db-$SUFFIX"
DESTINATION_PASSWORD="synthetic-destination-db-$SUFFIX"
read -r SOURCE_PORT DESTINATION_PORT < <(python3 -c '
import socket
sockets = [socket.socket(), socket.socket()]
for item in sockets: item.bind(("127.0.0.1", 0))
print(*(item.getsockname()[1] for item in sockets))
for item in sockets: item.close()
')

stack() {
  local role=$1
  shift
  if [[ "$role" == source ]]; then
    env DISPOSABLE_DB_PASSWORD="$SOURCE_PASSWORD" DISPOSABLE_HOST_PORT="$SOURCE_PORT" \
      DISPOSABLE_RUN_ID="$RUN_ID" docker compose --project-name "$SOURCE_PROJECT" \
      --file "$COMPOSE" "$@"
  else
    env DISPOSABLE_DB_PASSWORD="$DESTINATION_PASSWORD" \
      DISPOSABLE_HOST_PORT="$DESTINATION_PORT" DISPOSABLE_RUN_ID="$RUN_ID" \
      docker compose --project-name "$DESTINATION_PROJECT" --file "$COMPOSE" "$@"
  fi
}

cleanup() {
  set +e
  stack source down --volumes --remove-orphans >/dev/null 2>&1
  stack destination down --volumes --remove-orphans >/dev/null 2>&1
  case "$WORKSPACE" in /tmp/immich-rs-migration.*) find "$WORKSPACE" -depth -delete ;; esac
  unset IMMICH_RS_API_KEY IMMICH_RS_SOURCE_API_KEY IMMICH_RS_DESTINATION_API_KEY
  set -e
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

start_stack() {
  local role=$1 project=$2 port=$3
  stack "$role" up --detach --pull missing
  local container host_ip host_port
  container=$(stack "$role" ps --quiet server)
  read -r host_ip host_port < <(docker inspect --format \
    '{{with (index .HostConfig.PortBindings "2283/tcp")}}{{(index . 0).HostIp}} {{(index . 0).HostPort}}{{end}}' \
    "$container")
  [[ "$host_ip" == 127.0.0.1 && "$host_port" == "$port" ]] || {
    echo "$role server escaped its declared loopback binding" >&2
    exit 1
  }
  [[ $(docker inspect --format '{{index .Config.Labels "com.docker.compose.project"}}' "$container") == "$project" ]]
}

ready() {
  local endpoint=$1
  for _attempt in $(seq 1 120); do
    if curl --fail --silent --max-time 2 "$endpoint/api/server/ping" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

create_account() {
  local endpoint=$1 email=$2 password=$3
  local signup login
  signup=$(jq -nc --arg email "$email" --arg password "$password" \
    '{email:$email,password:$password,name:"Synthetic Migration Admin"}')
  curl --fail --silent --show-error --header 'Content-Type: application/json' \
    --request POST --data "$signup" "$endpoint/api/auth/admin-sign-up" >/dev/null
  login=$(jq -nc --arg email "$email" --arg password "$password" \
    '{email:$email,password:$password}')
  curl --fail --silent --show-error --header 'Content-Type: application/json' \
    --request POST --data "$login" "$endpoint/api/auth/login" \
    | jq -er '.accessToken | select(type == "string" and length > 0)'
}

create_key() {
  local endpoint=$1 token=$2 name=$3 permissions=$4
  local request
  request=$(jq -nc --arg name "$name" --argjson permissions "$permissions" \
    '{name:$name,permissions:$permissions}')
  curl --fail --silent --show-error --header 'Content-Type: application/json' \
    --header "Authorization: Bearer $token" --request POST --data "$request" \
    "$endpoint/api/api-keys"
}

echo 'starting two disposable Immich stacks' >&2
SOURCE_ENDPOINT="http://127.0.0.1:$SOURCE_PORT"
DESTINATION_ENDPOINT="http://127.0.0.1:$DESTINATION_PORT"
start_stack source "$SOURCE_PROJECT" "$SOURCE_PORT"
ready "$SOURCE_ENDPOINT" || {
  echo 'the disposable source Immich stack did not become ready' >&2
  exit 1
}
start_stack destination "$DESTINATION_PROJECT" "$DESTINATION_PORT"
ready "$DESTINATION_ENDPOINT" || {
  echo 'the disposable destination Immich stack did not become ready' >&2
  exit 1
}

echo 'creating synthetic accounts and scoped API keys' >&2
SOURCE_EMAIL="synthetic-source-$SUFFIX@example.invalid"
DESTINATION_EMAIL="synthetic-destination-$SUFFIX@example.invalid"
SOURCE_ADMIN=$(create_account "$SOURCE_ENDPOINT" "$SOURCE_EMAIL" "synthetic-source-$SUFFIX")
DESTINATION_ADMIN=$(create_account "$DESTINATION_ENDPOINT" "$DESTINATION_EMAIL" "synthetic-destination-$SUFFIX")
echo 'creating seed, read-only source and destination-write keys' >&2
SEED_PERMISSIONS='["asset.upload","asset.read","asset.update","album.read","album.create","albumAsset.create","server.about","user.read"]'
READ_PERMISSIONS='["asset.read","asset.download","album.read","server.about","user.read"]'
WRITE_PERMISSIONS='["asset.upload","asset.read","asset.update","album.read","album.create","albumAsset.create","server.about","user.read"]'
SEED_RESPONSE=$(create_key "$SOURCE_ENDPOINT" "$SOURCE_ADMIN" 'synthetic seed only' "$SEED_PERMISSIONS")
SOURCE_RESPONSE=$(create_key "$SOURCE_ENDPOINT" "$SOURCE_ADMIN" 'synthetic read-only migration' "$READ_PERMISSIONS")
DESTINATION_RESPONSE=$(create_key "$DESTINATION_ENDPOINT" "$DESTINATION_ADMIN" 'synthetic migration destination' "$WRITE_PERMISSIONS")
SEED_KEY_ID=$(jq -er '.apiKey.id // .id' <<<"$SEED_RESPONSE")
SEED_KEY=$(jq -er '.secret' <<<"$SEED_RESPONSE")
SOURCE_KEY=$(jq -er '.secret' <<<"$SOURCE_RESPONSE")
DESTINATION_KEY=$(jq -er '.secret' <<<"$DESTINATION_RESPONSE")

echo 'seeding only the synthetic source library' >&2
MANIFEST="$ROOT/tests/fixtures/v2/synthetic-google-takeout-complete/manifest.json"
SOURCE_FILES="$WORKSPACE/source-files"
python3 "$ROOT/scripts/materialize-fixture.py" "$MANIFEST" "$SOURCE_FILES"
SEED_PLAN="$WORKSPACE/seed-plan.json"
SEED_CHECKPOINT="$WORKSPACE/seed.sqlite"
IMMICH_RS_API_KEY="$SEED_KEY" "$BINARY" plan upload google-takeout \
  --server "$SOURCE_ENDPOINT" "$SOURCE_FILES" >"$SEED_PLAN"
SEED_REPORT=$(IMMICH_RS_API_KEY="$SEED_KEY" "$BINARY" apply upload \
  --server "$SOURCE_ENDPOINT" --plan "$SEED_PLAN" --source "$SOURCE_FILES" \
  --checkpoint "$SEED_CHECKPOINT")
[[ $(jq -r '.created' <<<"$SEED_REPORT") == 3 ]]
curl --fail --silent --header "Authorization: Bearer $SOURCE_ADMIN" \
  --request DELETE "$SOURCE_ENDPOINT/api/api-keys/$SEED_KEY_ID" >/dev/null
unset IMMICH_RS_API_KEY SEED_KEY SEED_RESPONSE

LIMITS=(--page-size 10 --max-assets 10 --max-albums 10 \
  --max-album-memberships 20 --max-asset-bytes 1048576 --max-total-bytes 4194304)
PLAN="$WORKSPACE/migration-plan.json"
PLAN_AFTER="$WORKSPACE/migration-plan-after.json"
CHECKPOINT="$WORKSPACE/migration.sqlite"
DUPLICATE_CHECKPOINT="$WORKSPACE/migration-duplicate.sqlite"
echo 'planning and applying the bounded two-server migration' >&2
IMMICH_RS_SOURCE_API_KEY="$SOURCE_KEY" IMMICH_RS_DESTINATION_API_KEY="$DESTINATION_KEY" \
  "$BINARY" plan migration immich --source-server "$SOURCE_ENDPOINT" \
  --destination-server "$DESTINATION_ENDPOINT" "${LIMITS[@]}" >"$PLAN"
SUMMARY=$(jq -c '.summary' "$PLAN")
EXPECTED_SUMMARY='{"assets":3,"media_bytes":214,"metadata_updates":3,"album_creates":1,"album_memberships":1,"max_mutations":8}'
[[ "$SUMMARY" == "$EXPECTED_SUMMARY" ]] || {
  echo "unexpected aggregate migration summary: $SUMMARY" >&2
  exit 1
}
echo 'validating offline dry-run' >&2
DRY_REPORT=$(env -u IMMICH_RS_SOURCE_API_KEY -u IMMICH_RS_DESTINATION_API_KEY \
  "$BINARY" apply migration immich --dry-run --plan "$PLAN" "${LIMITS[@]}")
[[ ! -e "$CHECKPOINT" && $(jq -r '.would_upload' <<<"$DRY_REPORT") == 3 ]]
APPLY=(apply migration immich --source-server "$SOURCE_ENDPOINT" \
  --destination-server "$DESTINATION_ENDPOINT" --plan "$PLAN" "${LIMITS[@]}")
echo 'applying first migration' >&2
FIRST_REPORT=$(IMMICH_RS_SOURCE_API_KEY="$SOURCE_KEY" \
  IMMICH_RS_DESTINATION_API_KEY="$DESTINATION_KEY" "$BINARY" "${APPLY[@]}" \
  --checkpoint "$CHECKPOINT")
echo 'resuming the durable checkpoint' >&2
RESUME_REPORT=$(IMMICH_RS_SOURCE_API_KEY="$SOURCE_KEY" \
  IMMICH_RS_DESTINATION_API_KEY="$DESTINATION_KEY" "$BINARY" "${APPLY[@]}" \
  --checkpoint "$CHECKPOINT")
echo 'proving fresh-checkpoint duplicate convergence' >&2
DUPLICATE_REPORT=$(IMMICH_RS_SOURCE_API_KEY="$SOURCE_KEY" \
  IMMICH_RS_DESTINATION_API_KEY="$DESTINATION_KEY" "$BINARY" "${APPLY[@]}" \
  --checkpoint "$DUPLICATE_CHECKPOINT")
[[ $(jq -r '.created' <<<"$FIRST_REPORT") == 3 ]]
[[ $(jq -r '.resumed_effects' <<<"$RESUME_REPORT") == 8 ]]
[[ $(jq -r '.duplicate' <<<"$DUPLICATE_REPORT") == 3 ]]
echo 'replanning to prove source immutability' >&2
IMMICH_RS_SOURCE_API_KEY="$SOURCE_KEY" IMMICH_RS_DESTINATION_API_KEY="$DESTINATION_KEY" \
  "$BINARY" plan migration immich --source-server "$SOURCE_ENDPOINT" \
  --destination-server "$DESTINATION_ENDPOINT" "${LIMITS[@]}" >"$PLAN_AFTER"
cmp --silent "$PLAN" "$PLAN_AFTER"
[[ -z $(find "$WORKSPACE" -maxdepth 1 -name '.immich-rs-stage-*' -print -quit) ]]
if grep -aF -e "$SOURCE_KEY" -e "$DESTINATION_KEY" -- "$PLAN" "$CHECKPOINT" \
  "$DUPLICATE_CHECKPOINT" >/dev/null; then
  echo 'a migration credential entered a durable artifact' >&2
  exit 1
fi

echo 'validating destination state and source immutability' >&2
SEARCH_REQUEST='{"size":100,"withExif":true}'
curl --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $DESTINATION_ADMIN" --request POST \
  --data "$SEARCH_REQUEST" "$DESTINATION_ENDPOINT/api/search/metadata" >"$WORKSPACE/search.json"
curl --fail --silent --get --header "Authorization: Bearer $DESTINATION_ADMIN" \
  --data 'isOwned=true' "$DESTINATION_ENDPOINT/api/albums" >"$WORKSPACE/albums.json"
ASSETS=$(jq -er '.assets.items | length' "$WORKSPACE/search.json")
ALBUMS=$(jq -er 'length' "$WORKSPACE/albums.json")
ALBUM_ID=$(jq -er '.[0].id' "$WORKSPACE/albums.json")
ALBUM_SEARCH=$(jq -nc --arg id "$ALBUM_ID" '{size:100,albumIds:[$id]}')
curl --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $DESTINATION_ADMIN" --request POST \
  --data "$ALBUM_SEARCH" "$DESTINATION_ENDPOINT/api/search/metadata" >"$WORKSPACE/album-assets.json"
MEMBERS=$(jq -er '.assets.items | length' "$WORKSPACE/album-assets.json")
DESCRIPTIONS=$(jq -r '[.assets.items[] | select(.exifInfo.description? != null and .exifInfo.description != "")] | length' "$WORKSPACE/search.json")
LOCATIONS=$(jq -r '[.assets.items[] | select(.exifInfo.latitude? != null and .exifInfo.longitude? != null)] | length' "$WORKSPACE/search.json")
if [[ "$ASSETS" != 3 || "$ALBUMS" != 1 || "$MEMBERS" != 1 \
  || "$DESCRIPTIONS" != 3 || "$LOCATIONS" != 1 ]]; then
  echo "unexpected destination aggregates: assets=$ASSETS albums=$ALBUMS members=$MEMBERS descriptions=$DESCRIPTIONS locations=$LOCATIONS" >&2
  exit 1
fi

if [[ -n "$BENCHMARK_OUTPUT" ]]; then
  echo 'running paired real-server migration benchmark' >&2
  BENCHMARK_MANIFEST="$ROOT/benchmarks/fixtures/phase8-takeout-64m.json"
  BENCHMARK_SOURCE="$WORKSPACE/benchmark-source"
  BENCHMARK_WORKSPACE="$WORKSPACE/benchmark-work"
  python3 "$ROOT/scripts/materialize-fixture.py" "$BENCHMARK_MANIFEST" "$BENCHMARK_SOURCE"
  mkdir -- "$BENCHMARK_WORKSPACE"
  IMMICH_RS_BENCHMARK_SOURCE_ADMIN_TOKEN="$SOURCE_ADMIN" \
    IMMICH_RS_BENCHMARK_DESTINATION_ADMIN_TOKEN="$DESTINATION_ADMIN" \
    python3 "$ROOT/scripts/benchmark-real-migration.py" \
      --source-endpoint "$SOURCE_ENDPOINT" --destination-endpoint "$DESTINATION_ENDPOINT" \
      --run-id "$SUFFIX" --source-revision "$COMMIT_SHA" --source "$BENCHMARK_SOURCE" \
      --fixture-manifest "$BENCHMARK_MANIFEST" --workspace "$BENCHMARK_WORKSPACE" \
      --immich-rs "$BINARY" --oracle "$ORACLE" --samples "$SAMPLES" \
      --warmups "$WARMUPS" --output "$BENCHMARK_OUTPUT"
fi

SERVER_VERSION=$(curl --fail --silent "$DESTINATION_ENDPOINT/api/server/version")
IMAGES=$(stack source config --format json | jq -c \
  '{server:.services.server.image,valkey:.services.redis.image,database:.services.database.image}')
BINARY_SHA256=$(sha256sum "$BINARY" | cut -d' ' -f1)
MANIFEST_SHA256=$(sha256sum "$MANIFEST" | cut -d' ' -f1)
PLAN_SHA256=$(sha256sum "$PLAN" | cut -d' ' -f1)
OPERATING_SYSTEM=$(uname -s)
ARCHITECTURE=$(uname -m)
DOCKER_VERSION=$(docker version --format '{{.Server.Version}}')
COMPOSE_VERSION=$(docker compose version --short)
cleanup
trap - EXIT HUP INT TERM
REMAINING_CONTAINERS=$(docker ps --all --quiet --filter "label=io.immich-rs.disposable.run=$RUN_ID")
REMAINING_VOLUMES=$(docker volume ls --quiet --filter "label=io.immich-rs.disposable.run=$RUN_ID")
REMAINING_NETWORKS=$(docker network ls --quiet --filter "label=io.immich-rs.disposable.run=$RUN_ID")
[[ -z "$REMAINING_CONTAINERS$REMAINING_VOLUMES$REMAINING_NETWORKS" ]]

jq -n --arg commit "$COMMIT_SHA" --arg binary_sha256 "$BINARY_SHA256" \
  --arg manifest_sha256 "$MANIFEST_SHA256" --arg plan_sha256 "$PLAN_SHA256" \
  --arg os "$OPERATING_SYSTEM" --arg arch "$ARCHITECTURE" \
  --arg docker "$DOCKER_VERSION" --arg compose "$COMPOSE_VERSION" \
  --argjson images "$IMAGES" --argjson version "$SERVER_VERSION" \
  --argjson summary "$SUMMARY" --argjson dry "$DRY_REPORT" \
  --argjson first "$FIRST_REPORT" --argjson resume "$RESUME_REPORT" \
  --argjson duplicate "$DUPLICATE_REPORT" --argjson assets "$ASSETS" \
  --argjson albums "$ALBUMS" --argjson members "$MEMBERS" \
  --argjson descriptions "$DESCRIPTIONS" --argjson locations "$LOCATIONS" \
  '{schema:"immich-migration-disposable-v1",commit_sha:$commit,binary_sha256:$binary_sha256,
    fixture:{kind:"synthetic",license:"CC0-1.0",manifest_sha256:$manifest_sha256},
    environment:{os:$os,architecture:$arch,docker:$docker,compose:$compose},images:$images,
    server_version:$version,isolation:{instances:2,published_loopback:true,production_endpoint:false,production_credentials:false},
    plan:{sha256:$plan_sha256,summary:$summary,source_byte_identical_after_apply:true},
    reports:{dry_run:$dry,first:$first,resume:$resume,fresh_checkpoint_duplicate:$duplicate},
    postconditions:{source_assets:3,source_owned_albums:1,destination_assets:$assets,
      destination_owned_albums:$albums,destination_album_members:$members,
      descriptions:$descriptions,locations:$locations},
    cleanup:{verified:true,labelled_containers:0,labelled_volumes:0,labelled_networks:0,
      staging_files:0,credentials_removed:true,seed_key_revoked:true}}' >"$OUTPUT"
