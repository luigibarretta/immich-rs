#!/usr/bin/env bash
set -euo pipefail

SERVER_IMAGE='ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa'
VALKEY_IMAGE='docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411'
DATABASE_IMAGE='ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23'
RESOURCE_LABEL='io.immich-rs.disposable.archive'

usage() {
  echo 'usage: run-disposable-archive.sh --binary <path> --commit-sha <sha> --output <path> [--oracle <path> --benchmark-output <path>]' >&2
  exit 2
}

BINARY=''
COMMIT_SHA=''
OUTPUT=''
ORACLE=''
BENCHMARK_OUTPUT=''
while (($# > 0)); do
  case "$1" in
    --binary) (($# >= 2)) || usage; BINARY=$2; shift 2 ;;
    --commit-sha) (($# >= 2)) || usage; COMMIT_SHA=$2; shift 2 ;;
    --output) (($# >= 2)) || usage; OUTPUT=$2; shift 2 ;;
    --oracle) (($# >= 2)) || usage; ORACLE=$2; shift 2 ;;
    --benchmark-output) (($# >= 2)) || usage; BENCHMARK_OUTPUT=$2; shift 2 ;;
    *) usage ;;
  esac
done
[[ -x "$BINARY" && "$COMMIT_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
BINARY=$(realpath -- "$BINARY")
if [[ -n "$ORACLE$BENCHMARK_OUTPUT" ]]; then
  [[ -x "$ORACLE" && -n "$BENCHMARK_OUTPUT" ]] || usage
  ORACLE=$(realpath -- "$ORACLE")
fi
REPOSITORY_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
OUTPUT=$(realpath -m -- "$OUTPUT")
case "$OUTPUT" in
  "$REPOSITORY_ROOT"/.artifacts/*) ;;
  *) echo 'evidence output must be inside .artifacts' >&2; exit 2 ;;
esac
[[ ! -e "$OUTPUT" ]] || { echo 'evidence output already exists' >&2; exit 2; }
mkdir -p -- "$REPOSITORY_ROOT/.artifacts"
if [[ -n "$BENCHMARK_OUTPUT" ]]; then
  BENCHMARK_OUTPUT=$(realpath -m -- "$BENCHMARK_OUTPUT")
  case "$BENCHMARK_OUTPUT" in
    "$REPOSITORY_ROOT"/.artifacts/*) ;;
    *) echo 'benchmark output must be inside .artifacts' >&2; exit 2 ;;
  esac
  [[ ! -e "$BENCHMARK_OUTPUT" ]] || { echo 'benchmark output already exists' >&2; exit 2; }
fi

RUN_SUFFIX="$(date -u +%Y%m%dT%H%M%SZ)-$$-$(openssl rand -hex 4)"
RUN_ID="phase5-$RUN_SUFFIX"
PREFIX="immich-rs-archive-$RUN_SUFFIX"
NETWORK="$PREFIX-network"
DATABASE="$PREFIX-database"
VALKEY="$PREFIX-valkey"
SERVER="$PREFIX-server"
GENERATOR="$PREFIX-generator"
DATABASE_VOLUME="$PREFIX-database"
UPLOAD_VOLUME="$PREFIX-upload"
WORKSPACE=$(mktemp -d '/tmp/immich-rs-archive.XXXXXX')
DRIVER_CONTAINER=''
PROXY_PID=''
CLEANED=0
declare -a PULLED_IMAGES=()

container_label() {
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
  for resource in "$GENERATOR" "$SERVER" "$VALKEY" "$DATABASE"; do
    if [[ $(container_label "$resource") == "$RUN_ID" ]]; then
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
    if [[ -z $(docker ps --all --quiet --filter "ancestor=$resource") ]]; then
      docker image rm "$resource" >/dev/null 2>&1
    fi
  done
  case "$WORKSPACE" in
    /tmp/immich-rs-archive.*) rm -rf -- "$WORKSPACE" ;;
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

docker network create --label "$RESOURCE_LABEL=$RUN_ID" \
  --opt com.docker.network.bridge.enable_ip_masquerade=false "$NETWORK" >/dev/null
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
declare -a PUBLICATION=(--publish 127.0.0.1::2283)
[[ -z "$DRIVER_CONTAINER" ]] || PUBLICATION=()
docker run --detach --name "$DATABASE" --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" --network-alias database --shm-size 128m \
  --env "POSTGRES_PASSWORD=$DATABASE_PASSWORD" --env POSTGRES_USER=postgres \
  --env POSTGRES_DB=immich --env POSTGRES_INITDB_ARGS=--data-checksums \
  --volume "$DATABASE_VOLUME:/var/lib/postgresql/data" "$DATABASE_IMAGE" >/dev/null
docker run --detach --name "$VALKEY" --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" --network-alias redis "$VALKEY_IMAGE" >/dev/null
docker run --detach --name "$SERVER" --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" "${PUBLICATION[@]}" --env DB_HOSTNAME=database \
  --env DB_USERNAME=postgres --env "DB_PASSWORD=$DATABASE_PASSWORD" \
  --env DB_DATABASE_NAME=immich --env REDIS_HOSTNAME=redis \
  --env IMMICH_LOG_LEVEL=warn --volume "$UPLOAD_VOLUME:/data" "$SERVER_IMAGE" >/dev/null

if [[ -n "$DRIVER_CONTAINER" ]]; then
  PROXY_PORT=$(python3 -c 'import socket; sock=socket.socket(); sock.bind(("127.0.0.1",0)); print(sock.getsockname()[1]); sock.close()')
  python3 "$REPOSITORY_ROOT/scripts/loopback-forward.py" \
    --listen-port "$PROXY_PORT" --target-container "$SERVER" &
  PROXY_PID=$!
  ENDPOINT="http://127.0.0.1:$PROXY_PORT"
else
  sleep 1
  PUBLISHED=$(docker port "$SERVER" 2283/tcp)
  [[ "$PUBLISHED" =~ ^127\.0\.0\.1:[0-9]+$ ]] || { echo 'server escaped loopback' >&2; exit 1; }
  ENDPOINT="http://$PUBLISHED"
fi
READY=0
for _attempt in $(seq 1 120); do
  if curl --fail --silent --max-time 2 "$ENDPOINT/api/server/ping" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done
[[ "$READY" == 1 ]] || { docker logs --tail 80 "$SERVER" >&2 || true; exit 1; }

EMAIL="synthetic-$RUN_SUFFIX@example.invalid"
PASSWORD="synthetic-admin-$RUN_SUFFIX"
SIGNUP=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email:$email,password:$password,name:"Synthetic Archive Admin"}')
curl --fail --silent --header 'Content-Type: application/json' --request POST \
  --data "$SIGNUP" "$ENDPOINT/api/auth/admin-sign-up" >/dev/null
LOGIN=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" '{email:$email,password:$password}')
ACCESS_TOKEN=$(curl --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$LOGIN" "$ENDPOINT/api/auth/login" | \
  jq -er '.accessToken | select(type == "string" and length > 0)')
KEY_RESPONSE=$(curl --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST \
  --data '{"name":"immich-rs disposable archive","permissions":["asset.upload","asset.read","asset.download","server.about","user.read"]}' \
  "$ENDPOINT/api/api-keys")
API_KEY=$(jq -er '.secret | select(type == "string" and length > 0)' <<<"$KEY_RESPONSE")

SOURCE="$WORKSPACE/source"
CORPUS_MANIFEST="$WORKSPACE/corpus.json"
UPLOAD_PLAN="$WORKSPACE/upload-plan.json"
CHECKPOINT="$WORKSPACE/checkpoint.sqlite"
ARCHIVE_MANIFEST="$WORKSPACE/archive-manifest.json"
DESTINATION="$WORKSPACE/archive"
"$REPOSITORY_ROOT/scripts/materialize-phase2-corpus.sh" \
  --image "$SERVER_IMAGE" --source "$SOURCE" --manifest "$CORPUS_MANIFEST" \
  --container "$GENERATOR" --run-label "$RESOURCE_LABEL=$RUN_ID"
echo 'disposable archive stage: seed synthetic assets' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" plan upload folder --server "$ENDPOINT" \
  --label synthetic-archive-seed "$SOURCE" >"$UPLOAD_PLAN"
UPLOAD_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload \
  --server "$ENDPOINT" --plan "$UPLOAD_PLAN" --source "$SOURCE" --checkpoint "$CHECKPOINT")
[[ $(jq -r '.created' <<<"$UPLOAD_REPORT") == 4 ]]

echo 'disposable archive stage: plan and archive originals' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" plan archive immich --server "$ENDPOINT" \
  --selection all --page-size 1 --max-assets 8 >"$ARCHIVE_MANIFEST"
FIRST_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply archive \
  --server "$ENDPOINT" --manifest "$ARCHIVE_MANIFEST" --destination "$DESTINATION")
SECOND_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply archive \
  --server "$ENDPOINT" --manifest "$ARCHIVE_MANIFEST" --destination "$DESTINATION")
ARCHIVE_ASSETS=$(jq -er '.summary.assets' "$ARCHIVE_MANIFEST")
FIRST_DOWNLOADED=$(jq -er '.downloaded' <<<"$FIRST_REPORT")
SECOND_COMPLETE=$(jq -er '.already_complete' <<<"$SECOND_REPORT")
if [[ "$ARCHIVE_ASSETS:$FIRST_DOWNLOADED:$SECOND_COMPLETE" != '4:4:4' ]]; then
  echo "unexpected archive counters: assets=$ARCHIVE_ASSETS first=$FIRST_DOWNLOADED second=$SECOND_COMPLETE" >&2
  exit 1
fi
[[ -z $(find "$DESTINATION" -type f -name '*.immich-rs.part' -print -quit) ]]
SOURCE_HASHES="$WORKSPACE/source-hashes"
ARCHIVE_HASHES="$WORKSPACE/archive-hashes"
for file in "$SOURCE/image.jpg" "$SOURCE/clip.mp4" "$SOURCE/live.jpg" "$SOURCE/live.mov"; do
  sha256sum "$file" | cut -d' ' -f1
done | sort >"$SOURCE_HASHES"
find "$DESTINATION/assets" -type f -print0 | sort -z | xargs -0 sha256sum | cut -d' ' -f1 | sort >"$ARCHIVE_HASHES"
cmp --silent "$SOURCE_HASHES" "$ARCHIVE_HASHES" || { echo 'archived bytes differ from sources' >&2; exit 1; }
if [[ -n "$BENCHMARK_OUTPUT" ]]; then
  echo 'disposable archive stage: paired benchmark' >&2
  BENCH_SOURCE="$WORKSPACE/benchmark-source"
  BENCH_FIXTURE="$WORKSPACE/benchmark-fixture.json"
  BENCH_PLAN="$WORKSPACE/benchmark-upload-plan.json"
  BENCH_CHECKPOINT="$WORKSPACE/benchmark-checkpoint.sqlite"
  "$REPOSITORY_ROOT/scripts/prepare-phase2-benchmark-corpus.sh" \
    --source "$SOURCE" --source-manifest "$CORPUS_MANIFEST" \
    --output "$BENCH_SOURCE" --output-manifest "$BENCH_FIXTURE"
  BENCH_EMAIL="synthetic-benchmark-$RUN_SUFFIX@example.invalid"
  BENCH_PASSWORD="synthetic-benchmark-$RUN_SUFFIX"
  BENCH_USER=$(jq -nc --arg email "$BENCH_EMAIL" --arg password "$BENCH_PASSWORD" \
    '{email:$email,password:$password,name:"Synthetic Archive Benchmark",shouldChangePassword:false}')
  curl --fail --silent --header 'Content-Type: application/json' \
    --header "Authorization: Bearer $ACCESS_TOKEN" --request POST \
    --data "$BENCH_USER" "$ENDPOINT/api/admin/users" >/dev/null
  BENCH_LOGIN=$(jq -nc --arg email "$BENCH_EMAIL" --arg password "$BENCH_PASSWORD" \
    '{email:$email,password:$password}')
  BENCH_TOKEN=$(curl --fail --silent --header 'Content-Type: application/json' \
    --request POST --data "$BENCH_LOGIN" "$ENDPOINT/api/auth/login" | \
    jq -er '.accessToken | select(type == "string" and length > 0)')
  BENCH_KEY=$(curl --fail --silent --header 'Content-Type: application/json' \
    --header "Authorization: Bearer $BENCH_TOKEN" --request POST \
    --data '{"name":"immich-rs paired archive","permissions":["album.read","asset.upload","asset.read","asset.download","server.about","user.read"]}' \
    "$ENDPOINT/api/api-keys" | jq -er '.secret | select(type == "string" and length > 0)')
  IMMICH_RS_API_KEY="$BENCH_KEY" "$BINARY" plan upload folder --server "$ENDPOINT" \
    --label synthetic-archive-benchmark "$BENCH_SOURCE" >"$BENCH_PLAN"
  IMMICH_RS_API_KEY="$BENCH_KEY" "$BINARY" apply upload --server "$ENDPOINT" \
    --plan "$BENCH_PLAN" --source "$BENCH_SOURCE" --checkpoint "$BENCH_CHECKPOINT" >/dev/null
  mkdir -- "$WORKSPACE/benchmark"
  IMMICH_RS_BENCHMARK_API_KEY="$BENCH_KEY" python3 \
    "$REPOSITORY_ROOT/scripts/benchmark-phase5.py" \
    --endpoint "$ENDPOINT" --source-revision "$COMMIT_SHA" \
    --source "$BENCH_SOURCE" --fixture-manifest "$BENCH_FIXTURE" \
    --workspace "$WORKSPACE/benchmark" --immich-rs "$BINARY" \
    --oracle "$ORACLE" --samples 6 --warmups 2 --output "$BENCHMARK_OUTPUT"
fi

SERVER_VERSION=$(curl --fail --silent "$ENDPOINT/api/server/version")
MANIFEST_SHA256=$(sha256sum "$ARCHIVE_MANIFEST" | cut -d' ' -f1)
BINARY_SHA256=$(sha256sum "$BINARY" | cut -d' ' -f1)
CORPUS_SHA256=$(sha256sum "$CORPUS_MANIFEST" | cut -d' ' -f1)
cleanup
CLEANED=1
trap - EXIT HUP INT TERM
REMAINING=$(docker ps --all --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING+=$(docker volume ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING+=$(docker network ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
[[ -z "$REMAINING" ]]
for image_ref in "${PULLED_IMAGES[@]}"; do
  ! docker image inspect "$image_ref" >/dev/null 2>&1 || { echo 'downloaded image remains' >&2; exit 1; }
done

jq -n --arg commit "$COMMIT_SHA" --arg binary_sha256 "$BINARY_SHA256" \
  --arg manifest_sha256 "$MANIFEST_SHA256" --arg corpus_sha256 "$CORPUS_SHA256" \
  --arg server_image "$SERVER_IMAGE" --arg valkey_image "$VALKEY_IMAGE" \
  --arg database_image "$DATABASE_IMAGE" --argjson server_version "$SERVER_VERSION" \
  --argjson first "$FIRST_REPORT" --argjson second "$SECOND_REPORT" \
  --argjson assets "$ARCHIVE_ASSETS" --argjson cleaned "$CLEANED" \
  '{schema:"phase5-disposable-archive-v1",commit_sha:$commit,environment:{os:"Linux",architecture:"x86_64",binary_sha256:$binary_sha256},images:{server:$server_image,valkey:$valkey_image,database:$database_image},fixture:{kind:"synthetic",license:"CC0-1.0",manifest_sha256:$corpus_sha256},archive_manifest_sha256:$manifest_sha256,server_version:$server_version,methodology:{network:"dedicated non-masquerading bridge; loopback-only client endpoint",commands:["seed synthetic folder upload","plan archive immich","apply archive","apply archive (verified resume)"],verification:"source and archived SHA-256 multisets are equal"},reports:{first:$first,second:$second},archive_assets:$assets,cleanup:{verified:($cleaned == 1),labelled_containers:0,labelled_volumes:0,labelled_networks:0}}' >"$OUTPUT"
