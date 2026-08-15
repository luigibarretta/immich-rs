#!/usr/bin/env bash
set -euo pipefail

SERVER_IMAGE='ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa'
VALKEY_IMAGE='docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411'
DATABASE_IMAGE='ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23'
RESOURCE_LABEL='io.immich-rs.disposable.run'

usage() {
  echo 'usage: run-disposable-immich.sh --binary <path> --commit-sha <sha> --output <path>' >&2
  exit 2
}

BINARY=''
COMMIT_SHA=''
OUTPUT=''
while (($# > 0)); do
  case "$1" in
    --binary)
      (($# >= 2)) || usage
      BINARY=$2
      shift 2
      ;;
    --commit-sha)
      (($# >= 2)) || usage
      COMMIT_SHA=$2
      shift 2
      ;;
    --output)
      (($# >= 2)) || usage
      OUTPUT=$2
      shift 2
      ;;
    *) usage ;;
  esac
done

[[ -f "$BINARY" && -x "$BINARY" ]] || usage
[[ "$COMMIT_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
REPOSITORY_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
OUTPUT=$(realpath -m -- "$OUTPUT")
case "$OUTPUT" in
  "$REPOSITORY_ROOT"/.artifacts/*) ;;
  *) echo 'evidence output must be inside .artifacts' >&2; exit 2 ;;
esac
[[ ! -e "$OUTPUT" ]] || { echo 'evidence output already exists' >&2; exit 2; }
mkdir -p -- "$REPOSITORY_ROOT/.artifacts"
BINARY_SHA256=$(sha256sum "$BINARY" | cut -d' ' -f1)
HOST_OS=$(uname -s)
HOST_ARCH=$(uname -m)
DOCKER_VERSION=$(docker version --format '{{.Server.Version}}')

RUN_SUFFIX="$(date -u +%Y%m%dT%H%M%SZ)-$$-$(openssl rand -hex 4)"
RUN_ID="phase2-$RUN_SUFFIX"
PREFIX="immich-rs-$RUN_SUFFIX"
NETWORK="$PREFIX-network"
DATABASE="$PREFIX-database"
VALKEY="$PREFIX-valkey"
SERVER="$PREFIX-server"
DATABASE_VOLUME="$PREFIX-database"
UPLOAD_VOLUME="$PREFIX-upload"
WORKSPACE=$(mktemp -d "/tmp/immich-rs-disposable.XXXXXX")
CLEANED=0
DRIVER_CONTAINER=''
PROXY_PID=''
declare -a PULLED_IMAGES=()

label_value() {
  docker inspect --format "{{ index .Config.Labels \"$RESOURCE_LABEL\" }}" "$1" 2>/dev/null || true
}

cleanup() {
  local resource label
  set +e
  if [[ -n "$PROXY_PID" ]]; then
    kill "$PROXY_PID" >/dev/null 2>&1
    wait "$PROXY_PID" >/dev/null 2>&1
    PROXY_PID=''
  fi
  for resource in "$SERVER" "$VALKEY" "$DATABASE"; do
    label=$(label_value "$resource")
    if [[ "$label" == "$RUN_ID" ]]; then
      docker rm --force --volumes "$resource" >/dev/null 2>&1
    fi
  done
  for resource in "$UPLOAD_VOLUME" "$DATABASE_VOLUME"; do
    label=$(docker volume inspect --format "{{ index .Labels \"$RESOURCE_LABEL\" }}" "$resource" 2>/dev/null || true)
    if [[ "$label" == "$RUN_ID" ]]; then
      docker volume rm "$resource" >/dev/null 2>&1
    fi
  done
  label=$(docker network inspect --format "{{ index .Labels \"$RESOURCE_LABEL\" }}" "$NETWORK" 2>/dev/null || true)
  if [[ "$label" == "$RUN_ID" ]]; then
    if [[ -n "$DRIVER_CONTAINER" ]]; then
      docker network disconnect --force "$NETWORK" "$DRIVER_CONTAINER" >/dev/null 2>&1
    fi
    docker network rm "$NETWORK" >/dev/null 2>&1
  fi
  for resource in "${PULLED_IMAGES[@]}"; do
    if [[ -z "$(docker ps --all --quiet --filter "ancestor=$resource")" ]]; then
      docker image rm "$resource" >/dev/null 2>&1
    fi
  done
  case "$WORKSPACE" in
    /tmp/immich-rs-disposable.*) rm -rf -- "$WORKSPACE" ;;
  esac
  set -e
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

ensure_image() {
  if ! docker image inspect "$1" >/dev/null 2>&1; then
    docker pull "$1" >/dev/null
    PULLED_IMAGES+=("$1")
  fi
}

ensure_image "$SERVER_IMAGE"
ensure_image "$VALKEY_IMAGE"
ensure_image "$DATABASE_IMAGE"

docker network create \
  --label "$RESOURCE_LABEL=$RUN_ID" \
  --opt com.docker.network.bridge.enable_ip_masquerade=false \
  "$NETWORK" >/dev/null
docker volume create --label "$RESOURCE_LABEL=$RUN_ID" "$DATABASE_VOLUME" >/dev/null
docker volume create --label "$RESOURCE_LABEL=$RUN_ID" "$UPLOAD_VOLUME" >/dev/null

if [[ -n "${HOSTNAME:-}" ]]; then
  candidate=$(docker inspect --format '{{.Id}}' "$HOSTNAME" 2>/dev/null || true)
  if [[ -n "$candidate" && "$candidate" == "$HOSTNAME"* ]]; then
    DRIVER_CONTAINER=$candidate
    docker network connect "$NETWORK" "$DRIVER_CONTAINER"
  fi
fi

DATABASE_PASSWORD="synthetic-db-$RUN_SUFFIX"
declare -a SERVER_PUBLICATION=(--publish 127.0.0.1::2283)
if [[ -n "$DRIVER_CONTAINER" ]]; then
  SERVER_PUBLICATION=()
fi
docker run --detach \
  --name "$DATABASE" \
  --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" \
  --network-alias database \
  --shm-size 128m \
  --env "POSTGRES_PASSWORD=$DATABASE_PASSWORD" \
  --env POSTGRES_USER=postgres \
  --env POSTGRES_DB=immich \
  --env POSTGRES_INITDB_ARGS=--data-checksums \
  --volume "$DATABASE_VOLUME:/var/lib/postgresql/data" \
  "$DATABASE_IMAGE" >/dev/null

docker run --detach \
  --name "$VALKEY" \
  --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" \
  --network-alias redis \
  "$VALKEY_IMAGE" >/dev/null

docker run --detach \
  --name "$SERVER" \
  --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" \
  "${SERVER_PUBLICATION[@]}" \
  --env DB_HOSTNAME=database \
  --env DB_USERNAME=postgres \
  --env "DB_PASSWORD=$DATABASE_PASSWORD" \
  --env DB_DATABASE_NAME=immich \
  --env REDIS_HOSTNAME=redis \
  --env IMMICH_LOG_LEVEL=warn \
  --volume "$UPLOAD_VOLUME:/data" \
  "$SERVER_IMAGE" >/dev/null

if [[ -n "$DRIVER_CONTAINER" ]]; then
  PROXY_PORT=$(python3 -c 'import socket; sock=socket.socket(); sock.bind(("127.0.0.1", 0)); print(sock.getsockname()[1]); sock.close()')
  python3 "$REPOSITORY_ROOT/scripts/loopback-forward.py" \
    --listen-port "$PROXY_PORT" --target-container "$SERVER" &
  PROXY_PID=$!
  ENDPOINT="http://127.0.0.1:$PROXY_PORT"
else
  sleep 1
  if ! PUBLISHED=$(docker port "$SERVER" 2283/tcp); then
    echo "disposable server state: $(docker inspect --format '{{.State.Status}}' "$SERVER")" >&2
    docker logs --tail 80 "$SERVER" >&2 || true
    exit 1
  fi
  [[ "$PUBLISHED" =~ ^127\.0\.0\.1:[0-9]+$ ]] || { echo 'server port escaped loopback' >&2; exit 1; }
  ENDPOINT="http://$PUBLISHED"
fi
READY=0
for _attempt in $(seq 1 120); do
  if curl --fail --silent --show-error --max-time 2 "$ENDPOINT/api/server/ping" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done
if [[ "$READY" != 1 ]]; then
  echo 'disposable Immich did not become ready' >&2
  docker logs --tail 80 "$DATABASE" >&2 || true
  docker logs --tail 80 "$SERVER" >&2 || true
  exit 1
fi

EMAIL="synthetic-$RUN_SUFFIX@example.invalid"
PASSWORD="synthetic-admin-$RUN_SUFFIX"
SIGNUP=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email:$email,password:$password,name:"Synthetic Disposable Admin"}')
curl --fail --silent --show-error \
  --header 'Content-Type: application/json' \
  --request POST \
  --data "$SIGNUP" \
  "$ENDPOINT/api/auth/admin-sign-up" >/dev/null
LOGIN_REQUEST=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email:$email,password:$password}')
LOGIN=$(curl --fail --silent --show-error \
  --header 'Content-Type: application/json' \
  --request POST \
  --data "$LOGIN_REQUEST" \
  "$ENDPOINT/api/auth/login")
ACCESS_TOKEN=$(jq -er '.accessToken | select(type == "string" and length > 0)' <<<"$LOGIN")
KEY_RESPONSE=$(curl --fail --silent --show-error \
  --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" \
  --request POST \
  --data '{"name":"immich-rs disposable","permissions":["asset.upload","user.read"]}' \
  "$ENDPOINT/api/api-keys")
API_KEY=$(jq -er '.secret | select(type == "string" and length > 0)' <<<"$KEY_RESPONSE")

SOURCE="$WORKSPACE/source"
PLAN="$WORKSPACE/upload-plan.json"
CHECKPOINT="$WORKSPACE/checkpoint.sqlite"
DUPLICATE_CHECKPOINT="$WORKSPACE/duplicate-checkpoint.sqlite"
mkdir -- "$SOURCE"
base64 --decode >"$SOURCE/synthetic.png" <<'PNG'
iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=
PNG
FIXTURE_SHA256=$(sha256sum "$SOURCE/synthetic.png" | cut -d' ' -f1)

echo 'disposable stage: plan' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" plan upload folder \
  --server "$ENDPOINT" \
  --label synthetic-disposable \
  "$SOURCE" >"$PLAN"
PLAN_SHA256=$(sha256sum "$PLAN" | cut -d' ' -f1)
echo 'disposable stage: dry-run' >&2
DRY_REPORT=$("$BINARY" apply upload --dry-run \
  --plan "$PLAN" --source "$SOURCE" --checkpoint "$CHECKPOINT")
[[ ! -e "$CHECKPOINT" ]] || { echo 'dry-run created a checkpoint' >&2; exit 1; }
echo 'disposable stage: first apply' >&2
FIRST_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload \
  --server "$ENDPOINT" --plan "$PLAN" --source "$SOURCE" --checkpoint "$CHECKPOINT")
echo 'disposable stage: resume apply' >&2
RESUME_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload \
  --server "$ENDPOINT" --plan "$PLAN" --source "$SOURCE" --checkpoint "$CHECKPOINT")
echo 'disposable stage: duplicate apply' >&2
DUPLICATE_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload \
  --server "$ENDPOINT" --plan "$PLAN" --source "$SOURCE" --checkpoint "$DUPLICATE_CHECKPOINT")

[[ $(jq -r '.would_upload' <<<"$DRY_REPORT") == 1 ]]
[[ $(jq -r '.created' <<<"$FIRST_REPORT") == 1 ]]
[[ $(jq -r '.resumed' <<<"$RESUME_REPORT") == 1 ]]
[[ $(jq -r '.duplicate' <<<"$DUPLICATE_REPORT") == 1 ]]
STATISTICS=$(curl --fail --silent --show-error \
  --header "Authorization: Bearer $ACCESS_TOKEN" \
  "$ENDPOINT/api/assets/statistics")
ASSET_COUNT=$(jq -er '.total' <<<"$STATISTICS")
[[ "$ASSET_COUNT" == 1 ]] || { echo 'disposable asset count did not converge' >&2; exit 1; }
SERVER_VERSION=$(curl --fail --silent --show-error "$ENDPOINT/api/server/version")

cleanup
CLEANED=1
trap - EXIT HUP INT TERM
REMAINING_CONTAINERS=$(docker ps --all --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING_VOLUMES=$(docker volume ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING_NETWORKS=$(docker network ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
[[ -z "$REMAINING_CONTAINERS$REMAINING_VOLUMES$REMAINING_NETWORKS" ]]
for image_ref in "${PULLED_IMAGES[@]}"; do
  if docker image inspect "$image_ref" >/dev/null 2>&1; then
    echo 'a disposable image downloaded by this run remains present' >&2
    exit 1
  fi
done

jq -n \
  --arg commit "$COMMIT_SHA" \
  --arg binary_sha256 "$BINARY_SHA256" \
  --arg host_os "$HOST_OS" \
  --arg host_arch "$HOST_ARCH" \
  --arg docker_version "$DOCKER_VERSION" \
  --arg server_image "$SERVER_IMAGE" \
  --arg valkey_image "$VALKEY_IMAGE" \
  --arg database_image "$DATABASE_IMAGE" \
  --arg fixture_sha256 "$FIXTURE_SHA256" \
  --arg plan_sha256 "$PLAN_SHA256" \
  --argjson server_version "$SERVER_VERSION" \
  --argjson dry_run "$DRY_REPORT" \
  --argjson first "$FIRST_REPORT" \
  --argjson resume "$RESUME_REPORT" \
  --argjson duplicate "$DUPLICATE_REPORT" \
  --argjson asset_count "$ASSET_COUNT" \
  --argjson cleanup_verified "$CLEANED" \
  --argjson downloaded_images "${#PULLED_IMAGES[@]}" \
  '{schema:"phase2-disposable-v1",commit_sha:$commit,environment:{os:$host_os,architecture:$host_arch,docker_server_version:$docker_version,binary_sha256:$binary_sha256},images:{server:$server_image,valkey:$valkey_image,database:$database_image},fixture:{kind:"synthetic",license:"CC0-1.0",sha256:$fixture_sha256,assets:1},plan_sha256:$plan_sha256,methodology:{network:"dedicated non-masquerading bridge; ephemeral 127.0.0.1 endpoint",commands:["plan upload folder","apply upload --dry-run","apply upload","apply upload (checkpoint resume)","apply upload (fresh checkpoint duplicate check)"]},server_version:$server_version,reports:{dry_run:$dry_run,first:$first,resume:$resume,duplicate:$duplicate},asset_count:$asset_count,cleanup:{verified:($cleanup_verified == 1),labelled_containers:0,labelled_volumes:0,labelled_networks:0,downloaded_images_removed:$downloaded_images}}' \
  >"$OUTPUT"
