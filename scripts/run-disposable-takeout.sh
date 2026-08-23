#!/usr/bin/env bash
set -euo pipefail

SERVER_IMAGE='ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa'
VALKEY_IMAGE='docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411'
DATABASE_IMAGE='ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23'
RESOURCE_LABEL='io.immich-rs.disposable.run'

usage() {
  echo 'usage: run-disposable-takeout.sh --binary <path> --commit-sha <sha> --output <path>' >&2
  exit 2
}

BINARY=''
COMMIT_SHA=''
OUTPUT=''
while (($# > 0)); do
  case "$1" in
    --binary) (($# >= 2)) || usage; BINARY=$2; shift 2 ;;
    --commit-sha) (($# >= 2)) || usage; COMMIT_SHA=$2; shift 2 ;;
    --output) (($# >= 2)) || usage; OUTPUT=$2; shift 2 ;;
    *) usage ;;
  esac
done
[[ -f "$BINARY" && -x "$BINARY" && "$COMMIT_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
BINARY=$(realpath -- "$BINARY")
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
OUTPUT=$(realpath -m -- "$OUTPUT")
case "$OUTPUT" in "$ROOT"/.artifacts/*) ;; *) usage ;; esac
[[ ! -e "$OUTPUT" ]] || { echo 'evidence output already exists' >&2; exit 2; }
mkdir -p -- "$ROOT/.artifacts"

SUFFIX="$(date -u +%Y%m%dT%H%M%SZ)-$$-$(openssl rand -hex 4)"
RUN_ID="phase8-$SUFFIX"
PREFIX="immich-rs-$SUFFIX"
NETWORK="$PREFIX-network"
DATABASE="$PREFIX-database"
VALKEY="$PREFIX-valkey"
SERVER="$PREFIX-server"
DATABASE_VOLUME="$PREFIX-database"
UPLOAD_VOLUME="$PREFIX-upload"
WORKSPACE=$(mktemp -d "/tmp/immich-rs-takeout.XXXXXX")
DRIVER_CONTAINER=''
HTTP_PROXY_PID=''
TLS_PROXY_PID=''
CLEANED=0
declare -a PULLED_IMAGES=()

container_label() {
  docker inspect --format "{{ index .Config.Labels \"$RESOURCE_LABEL\" }}" "$1" 2>/dev/null || true
}

cleanup() {
  local resource label
  set +e
  for resource in "$TLS_PROXY_PID" "$HTTP_PROXY_PID"; do
    if [[ -n "$resource" ]]; then kill "$resource" >/dev/null 2>&1; wait "$resource" >/dev/null 2>&1; fi
  done
  for resource in "$SERVER" "$VALKEY" "$DATABASE"; do
    if [[ $(container_label "$resource") == "$RUN_ID" ]]; then
      docker rm --force --volumes "$resource" >/dev/null 2>&1
    fi
  done
  for resource in "$UPLOAD_VOLUME" "$DATABASE_VOLUME"; do
    label=$(docker volume inspect --format "{{ index .Labels \"$RESOURCE_LABEL\" }}" "$resource" 2>/dev/null || true)
    if [[ "$label" == "$RUN_ID" ]]; then docker volume rm "$resource" >/dev/null 2>&1; fi
  done
  label=$(docker network inspect --format "{{ index .Labels \"$RESOURCE_LABEL\" }}" "$NETWORK" 2>/dev/null || true)
  if [[ "$label" == "$RUN_ID" ]]; then
    if [[ -n "$DRIVER_CONTAINER" ]]; then docker network disconnect --force "$NETWORK" "$DRIVER_CONTAINER" >/dev/null 2>&1; fi
    docker network rm "$NETWORK" >/dev/null 2>&1
  fi
  for resource in "${PULLED_IMAGES[@]}"; do
    if [[ -z $(docker ps --all --quiet --filter "ancestor=$resource") ]]; then docker image rm "$resource" >/dev/null 2>&1; fi
  done
  case "$WORKSPACE" in /tmp/immich-rs-takeout.*) rm -rf -- "$WORKSPACE" ;; esac
  set -e
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

ensure_image() {
  if ! docker image inspect "$1" >/dev/null 2>&1; then docker pull "$1" >/dev/null; PULLED_IMAGES+=("$1"); fi
}
for image in "$SERVER_IMAGE" "$VALKEY_IMAGE" "$DATABASE_IMAGE"; do ensure_image "$image"; done

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

DATABASE_PASSWORD="synthetic-db-$SUFFIX"
declare -a PUBLICATION=(--publish 127.0.0.1::2283)
if [[ -n "$DRIVER_CONTAINER" ]]; then PUBLICATION=(); fi
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
  --env DB_DATABASE_NAME=immich --env REDIS_HOSTNAME=redis --env IMMICH_LOG_LEVEL=warn \
  --volume "$UPLOAD_VOLUME:/data" "$SERVER_IMAGE" >/dev/null

if [[ -n "$DRIVER_CONTAINER" ]]; then
  HTTP_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')
  python3 "$ROOT/scripts/loopback-forward.py" --listen-port "$HTTP_PORT" --target-container "$SERVER" &
  HTTP_PROXY_PID=$!
  TLS_HOST=$(docker inspect --format "{{with index .NetworkSettings.Networks \"$NETWORK\"}}{{.IPAddress}}{{end}}" "$DRIVER_CONTAINER")
else
  sleep 1
  PUBLISHED=$(docker port "$SERVER" 2283/tcp)
  [[ "$PUBLISHED" =~ ^127\.0\.0\.1:([0-9]+)$ ]] || { echo 'server port escaped loopback' >&2; exit 1; }
  HTTP_PORT=${BASH_REMATCH[1]}
  TLS_HOST=$(docker network inspect --format '{{(index .IPAM.Config 0).Gateway}}' "$NETWORK")
fi
TLS_PORT=$(python3 -c 'import socket,sys; s=socket.socket(); s.bind((sys.argv[1],0)); print(s.getsockname()[1]); s.close()' "$TLS_HOST")
CA_KEY="$WORKSPACE/ca-key.pem"
CA_CERTIFICATE="$WORKSPACE/ca.pem"
SERVER_KEY="$WORKSPACE/server-key.pem"
openssl ecparam -name prime256v1 -genkey -noout -out "$CA_KEY" 2>/dev/null
openssl req -x509 -new -sha256 -key "$CA_KEY" -days 1 -subj '/CN=immich-rs disposable CA' -out "$CA_CERTIFICATE" 2>/dev/null
openssl ecparam -name prime256v1 -genkey -noout -out "$SERVER_KEY" 2>/dev/null
openssl req -new -sha256 -key "$SERVER_KEY" -subj '/CN=immich-rs disposable HTTPS' -out "$WORKSPACE/server.csr" 2>/dev/null
printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\n' "$TLS_HOST" >"$WORKSPACE/extensions.cnf"
openssl x509 -req -sha256 -in "$WORKSPACE/server.csr" -CA "$CA_CERTIFICATE" -CAkey "$CA_KEY" \
  -CAcreateserial -days 1 -extfile "$WORKSPACE/extensions.cnf" -out "$WORKSPACE/server.pem" 2>/dev/null
chmod 0600 "$CA_KEY" "$SERVER_KEY"
python3 "$ROOT/scripts/tls-forward.py" --listen-host "$TLS_HOST" --listen-port "$TLS_PORT" \
  --target-port "$HTTP_PORT" --certificate "$WORKSPACE/server.pem" --private-key "$SERVER_KEY" &
TLS_PROXY_PID=$!
ENDPOINT="https://$TLS_HOST:$TLS_PORT"
READY=0
for _attempt in $(seq 1 120); do
  if curl --cacert "$CA_CERTIFICATE" --fail --silent --max-time 2 "$ENDPOINT/api/server/ping" >/dev/null 2>&1; then READY=1; break; fi
  sleep 1
done
[[ "$READY" == 1 ]] || { echo 'disposable HTTPS Immich did not become ready' >&2; exit 1; }

EMAIL="synthetic-$SUFFIX@example.invalid"
PASSWORD="synthetic-admin-$SUFFIX"
SIGNUP=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" '{email:$email,password:$password,name:"Synthetic Disposable Admin"}')
curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$SIGNUP" "$ENDPOINT/api/auth/admin-sign-up" >/dev/null
LOGIN_REQUEST=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" '{email:$email,password:$password}')
LOGIN=$(curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$LOGIN_REQUEST" "$ENDPOINT/api/auth/login")
ACCESS_TOKEN=$(jq -er '.accessToken | select(type == "string" and length > 0)' <<<"$LOGIN")
PERMISSIONS='["asset.upload","asset.read","asset.update","album.read","album.create","albumAsset.create","server.about","user.read"]'
KEY_REQUEST=$(jq -nc --argjson permissions "$PERMISSIONS" '{name:"immich-rs disposable Takeout",permissions:$permissions}')
KEY_RESPONSE=$(curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST --data "$KEY_REQUEST" "$ENDPOINT/api/api-keys")
API_KEY=$(jq -er '.secret | select(type == "string" and length > 0)' <<<"$KEY_RESPONSE")

MANIFEST="$ROOT/tests/fixtures/v2/synthetic-google-takeout-complete/manifest.json"
DIRECTORY="$WORKSPACE/directory"
ARCHIVES="$WORKSPACE/archives"
python3 "$ROOT/scripts/materialize-fixture.py" "$MANIFEST" "$DIRECTORY"
python3 "$ROOT/scripts/materialize-fixture.py" "$MANIFEST" "$ARCHIVES" --archive-view split
MANIFEST_SHA256=$(sha256sum "$MANIFEST" | cut -d' ' -f1)
DIRECTORY_PLAN="$WORKSPACE/directory-plan.json"
ARCHIVE_PLAN="$WORKSPACE/archive-plan.json"
COMMON_PLAN=(plan upload google-takeout --server "$ENDPOINT" --authorize-production-read \
  --ca-certificate "$CA_CERTIFICATE" --label synthetic-google-takeout-complete)
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${COMMON_PLAN[@]}" "$DIRECTORY" >"$DIRECTORY_PLAN"
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${COMMON_PLAN[@]}" \
  "$ARCHIVES/takeout-001.zip" "$ARCHIVES/takeout-002.zip" >"$ARCHIVE_PLAN"
cmp --silent "$DIRECTORY_PLAN" "$ARCHIVE_PLAN" || { echo 'directory and ZIP plans differ' >&2; exit 1; }
INSPECTION=$("$BINARY" inspect upload-plan --plan "$DIRECTORY_PLAN")
PLAN_SHA256=$(jq -er '.plan_sha256' <<<"$INSPECTION")
OPERATIONS=$(jq -er '.summary.operations' "$DIRECTORY_PLAN")
MAX_MUTATIONS=$(jq -er '.summary.max_mutations' "$DIRECTORY_PLAN")
[[ "$OPERATIONS" == 3 && "$MAX_MUTATIONS" == 8 ]]

CHECKPOINT="$WORKSPACE/directory.sqlite"
ARCHIVE_CHECKPOINT="$WORKSPACE/archive.sqlite"
DRY_REPORT=$("$BINARY" apply upload --dry-run --plan "$DIRECTORY_PLAN" \
  --source "$DIRECTORY" --checkpoint "$CHECKPOINT")
[[ ! -e "$CHECKPOINT" && $(jq -r '.would_upload' <<<"$DRY_REPORT") == 3 ]]
BACKUP_REFERENCE="synthetic-backup-$SUFFIX"
PRODUCTION=(--server "$ENDPOINT" --ca-certificate "$CA_CERTIFICATE" \
  --authorize-production-read --authorize-production-write --confirm-plan-sha256 "$PLAN_SHA256" \
  --expected-operations "$MAX_MUTATIONS" --backup-reference "$BACKUP_REFERENCE")
set +e
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload "${PRODUCTION[@]}" \
  --confirm-plan-sha256 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
  --plan "$DIRECTORY_PLAN" --source "$DIRECTORY" --checkpoint "$CHECKPOINT" >/dev/null 2>"$WORKSPACE/mismatch.stderr"
MISMATCH_EXIT=$?
set -e
[[ "$MISMATCH_EXIT" == 2 && ! -e "$CHECKPOINT" ]]

FIRST_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload "${PRODUCTION[@]}" \
  --plan "$DIRECTORY_PLAN" --source "$DIRECTORY" --checkpoint "$CHECKPOINT")
RESUME_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload "${PRODUCTION[@]}" \
  --plan "$DIRECTORY_PLAN" --source "$DIRECTORY" --checkpoint "$CHECKPOINT")
ARCHIVE_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" apply upload "${PRODUCTION[@]}" \
  --plan "$ARCHIVE_PLAN" --input "$ARCHIVES/takeout-001.zip" --input "$ARCHIVES/takeout-002.zip" \
  --checkpoint "$ARCHIVE_CHECKPOINT")
[[ $(jq -r '.created' <<<"$FIRST_REPORT") == 3 ]]
[[ $(jq -r '.resumed_effects' <<<"$RESUME_REPORT") == 8 ]]
[[ $(jq -r '.duplicate' <<<"$ARCHIVE_REPORT") == 3 ]]
[[ -z $(find "$WORKSPACE" -maxdepth 1 -name '.immich-rs-stage-*' -print -quit) ]]
if grep -aF -- "$BACKUP_REFERENCE" "$CHECKPOINT" "$ARCHIVE_CHECKPOINT" >/dev/null; then
  echo 'raw backup reference entered a checkpoint' >&2; exit 1
fi
if grep -aF -- "$API_KEY" "$DIRECTORY_PLAN" "$ARCHIVE_PLAN" "$CHECKPOINT" "$ARCHIVE_CHECKPOINT" >/dev/null; then
  echo 'API key entered an artifact' >&2; exit 1
fi

curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST --data '{"size":100,"withExif":true}' \
  "$ENDPOINT/api/search/metadata" >"$WORKSPACE/search.json"
curl --cacert "$CA_CERTIFICATE" --fail --silent --get --header "Authorization: Bearer $ACCESS_TOKEN" \
  --data-urlencode 'name=Synthetic Album' --data 'isOwned=true' "$ENDPOINT/api/albums" >"$WORKSPACE/albums.json"
ALBUM_ID=$(jq -er 'if length == 1 then .[0].id else error("album count") end' "$WORKSPACE/albums.json")
ALBUM_SEARCH=$(jq -nc --arg id "$ALBUM_ID" '{size:100,albumIds:[$id]}')
curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --header "Authorization: Bearer $ACCESS_TOKEN" --request POST --data "$ALBUM_SEARCH" \
  "$ENDPOINT/api/search/metadata" >"$WORKSPACE/album-assets.json"
POSTCONDITIONS=$(python3 "$ROOT/scripts/verify-takeout-postconditions.py" \
  --search "$WORKSPACE/search.json" --albums "$WORKSPACE/albums.json" \
  --album-assets "$WORKSPACE/album-assets.json")
SERVER_VERSION=$(curl --cacert "$CA_CERTIFICATE" --fail --silent "$ENDPOINT/api/server/version")
CA_SHA256=$(sha256sum "$CA_CERTIFICATE" | cut -d' ' -f1)
BINARY_SHA256=$(sha256sum "$BINARY" | cut -d' ' -f1)

cleanup
CLEANED=1
trap - EXIT HUP INT TERM
REMAINING_CONTAINERS=$(docker ps --all --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING_VOLUMES=$(docker volume ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING_NETWORKS=$(docker network ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
[[ -z "$REMAINING_CONTAINERS$REMAINING_VOLUMES$REMAINING_NETWORKS" ]]

jq -n --arg commit "$COMMIT_SHA" --arg binary_sha256 "$BINARY_SHA256" \
  --arg fixture_manifest_sha256 "$MANIFEST_SHA256" --arg plan_sha256 "$PLAN_SHA256" \
  --arg server_image "$SERVER_IMAGE" --arg valkey_image "$VALKEY_IMAGE" --arg database_image "$DATABASE_IMAGE" \
  --arg ca_sha256 "$CA_SHA256" --argjson server_version "$SERVER_VERSION" \
  --argjson dry_run "$DRY_REPORT" --argjson first "$FIRST_REPORT" --argjson resume "$RESUME_REPORT" \
  --argjson archive "$ARCHIVE_REPORT" --argjson postconditions "$POSTCONDITIONS" \
  --argjson cleanup_verified "$CLEANED" --argjson mismatch_exit "$MISMATCH_EXIT" \
  '{schema:"phase8-disposable-takeout-v1",commit_sha:$commit,binary_sha256:$binary_sha256,
    fixture:{schema:"fixture-manifest-v2",manifest_sha256:$fixture_manifest_sha256,provenance:"synthetic-CC0-1.0"},
    images:{server:$server_image,valkey:$valkey_image,database:$database_image},
    tls:{custom_ca_sha256:$ca_sha256,hostname_verification:true},server_version:$server_version,
    plan:{directory_zip_byte_identical:true,sha256:$plan_sha256,operations:3,media_bytes:214,xmp_sidecars:0,live_photo_pairs:0,metadata_updates:3,album_creates:1,album_memberships:1,max_mutations:8},
    authorization:{maximum_mutation_budget:8,backup_reference_hashed:true,mismatch_exit:$mismatch_exit},
    reports:{dry_run:$dry_run,first:$first,resume:$resume,archive_duplicate:$archive},postconditions:$postconditions,
    cleanup:{verified:($cleanup_verified == 1),labelled_containers:0,labelled_volumes:0,labelled_networks:0,staging_files:0,private_keys_removed:true,credentials_removed:true}}' >"$OUTPUT"
