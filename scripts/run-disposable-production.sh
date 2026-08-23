#!/usr/bin/env bash
set -euo pipefail

SERVER_IMAGE='ghcr.io/immich-app/immich-server@sha256:079cc990b26a88d71f96027341c67329cb11829d4c341ce33b3718fe0f84cbfa'
VALKEY_IMAGE='docker.io/valkey/valkey:9@sha256:8e8d64b405ce18f41b8e5ee20aa4687a8ed0022d1298f2ce31cdcf3a76e09411'
DATABASE_IMAGE='ghcr.io/immich-app/postgres:14-vectorchord0.4.3-pgvectors0.2.0@sha256:bcf63357191b76a916ae5eb93464d65c07511da41e3bf7a8416db519b40b1c23'
RESOURCE_LABEL='io.immich-rs.disposable.run'

usage() {
  echo 'usage: run-disposable-production.sh --binary <path> --commit-sha <sha> --output <path>' >&2
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

[[ -f "$BINARY" && -x "$BINARY" ]] || usage
BINARY=$(realpath -- "$BINARY")
[[ "$COMMIT_SHA" =~ ^[0-9a-f]{40}$ ]] || usage
REPOSITORY_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
OUTPUT=$(realpath -m -- "$OUTPUT")
case "$OUTPUT" in
  "$REPOSITORY_ROOT"/.artifacts/*) ;;
  *) echo 'evidence output must be inside .artifacts' >&2; exit 2 ;;
esac
[[ ! -e "$OUTPUT" ]] || { echo 'evidence output already exists' >&2; exit 2; }
mkdir -p -- "$REPOSITORY_ROOT/.artifacts"

RUN_SUFFIX="$(date -u +%Y%m%dT%H%M%SZ)-$$-$(openssl rand -hex 4)"
RUN_ID="phase7-$RUN_SUFFIX"
PREFIX="immich-rs-$RUN_SUFFIX"
NETWORK="$PREFIX-network"
DATABASE="$PREFIX-database"
VALKEY="$PREFIX-valkey"
SERVER="$PREFIX-server"
GENERATOR="$PREFIX-generator"
DATABASE_VOLUME="$PREFIX-database"
UPLOAD_VOLUME="$PREFIX-upload"
WORKSPACE=$(mktemp -d "/tmp/immich-rs-production.XXXXXX")
DRIVER_CONTAINER=''
HTTP_PROXY_PID=''
TLS_PROXY_PID=''
CLEANED=0
declare -a PULLED_IMAGES=()

label_value() {
  docker inspect --format "{{ index .Config.Labels \"$RESOURCE_LABEL\" }}" "$1" 2>/dev/null || true
}

cleanup() {
  local resource label
  set +e
  for resource in "$TLS_PROXY_PID" "$HTTP_PROXY_PID"; do
    if [[ -n "$resource" ]]; then
      kill "$resource" >/dev/null 2>&1
      wait "$resource" >/dev/null 2>&1
    fi
  done
  for resource in "$GENERATOR" "$SERVER" "$VALKEY" "$DATABASE"; do
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
    /tmp/immich-rs-production.*) rm -rf -- "$WORKSPACE" ;;
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

DATABASE_PASSWORD="synthetic-db-$RUN_SUFFIX"
declare -a SERVER_PUBLICATION=(--publish 127.0.0.1::2283)
if [[ -n "$DRIVER_CONTAINER" ]]; then SERVER_PUBLICATION=(); fi
docker run --detach --name "$DATABASE" --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" --network-alias database --shm-size 128m \
  --env "POSTGRES_PASSWORD=$DATABASE_PASSWORD" --env POSTGRES_USER=postgres \
  --env POSTGRES_DB=immich --env POSTGRES_INITDB_ARGS=--data-checksums \
  --volume "$DATABASE_VOLUME:/var/lib/postgresql/data" "$DATABASE_IMAGE" >/dev/null
docker run --detach --name "$VALKEY" --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" --network-alias redis "$VALKEY_IMAGE" >/dev/null
docker run --detach --name "$SERVER" --label "$RESOURCE_LABEL=$RUN_ID" \
  --network "$NETWORK" "${SERVER_PUBLICATION[@]}" \
  --env DB_HOSTNAME=database --env DB_USERNAME=postgres \
  --env "DB_PASSWORD=$DATABASE_PASSWORD" --env DB_DATABASE_NAME=immich \
  --env REDIS_HOSTNAME=redis --env IMMICH_LOG_LEVEL=warn \
  --volume "$UPLOAD_VOLUME:/data" "$SERVER_IMAGE" >/dev/null

if [[ -n "$DRIVER_CONTAINER" ]]; then
  HTTP_PORT=$(python3 -c 'import socket; sock=socket.socket(); sock.bind(("127.0.0.1",0)); print(sock.getsockname()[1]); sock.close()')
  python3 "$REPOSITORY_ROOT/scripts/loopback-forward.py" \
    --listen-port "$HTTP_PORT" --target-container "$SERVER" &
  HTTP_PROXY_PID=$!
  TLS_HOST=$(docker inspect --format "{{with index .NetworkSettings.Networks \"$NETWORK\"}}{{.IPAddress}}{{end}}" "$DRIVER_CONTAINER")
else
  sleep 1
  PUBLISHED=$(docker port "$SERVER" 2283/tcp)
  [[ "$PUBLISHED" =~ ^127\.0\.0\.1:([0-9]+)$ ]] || { echo 'server port escaped loopback' >&2; exit 1; }
  HTTP_PORT=${BASH_REMATCH[1]}
  TLS_HOST=$(docker network inspect --format '{{(index .IPAM.Config 0).Gateway}}' "$NETWORK")
fi
TLS_PORT=$(python3 -c 'import socket,sys; sock=socket.socket(); sock.bind((sys.argv[1],0)); print(sock.getsockname()[1]); sock.close()' "$TLS_HOST")

CA_KEY="$WORKSPACE/ca-key.pem"
CA_CERTIFICATE="$WORKSPACE/ca-certificate.pem"
SERVER_KEY="$WORKSPACE/server-key.pem"
SERVER_REQUEST="$WORKSPACE/server.csr"
SERVER_CERTIFICATE="$WORKSPACE/server-certificate.pem"
EXTENSIONS="$WORKSPACE/server-extensions.cnf"
openssl ecparam -name prime256v1 -genkey -noout -out "$CA_KEY" 2>/dev/null
openssl req -x509 -new -sha256 -key "$CA_KEY" -days 1 \
  -subj '/CN=immich-rs disposable CA' -out "$CA_CERTIFICATE" 2>/dev/null
openssl ecparam -name prime256v1 -genkey -noout -out "$SERVER_KEY" 2>/dev/null
openssl req -new -sha256 -key "$SERVER_KEY" -subj '/CN=immich-rs disposable HTTPS' \
  -out "$SERVER_REQUEST" 2>/dev/null
printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\n' "$TLS_HOST" >"$EXTENSIONS"
openssl x509 -req -sha256 -in "$SERVER_REQUEST" -CA "$CA_CERTIFICATE" -CAkey "$CA_KEY" \
  -CAcreateserial -days 1 -extfile "$EXTENSIONS" -out "$SERVER_CERTIFICATE" 2>/dev/null
chmod 0600 "$CA_KEY" "$SERVER_KEY"
python3 "$REPOSITORY_ROOT/scripts/tls-forward.py" --listen-host "$TLS_HOST" \
  --listen-port "$TLS_PORT" --target-port "$HTTP_PORT" \
  --certificate "$SERVER_CERTIFICATE" --private-key "$SERVER_KEY" &
TLS_PROXY_PID=$!
ENDPOINT="https://$TLS_HOST:$TLS_PORT"

READY=0
for _attempt in $(seq 1 120); do
  if curl --cacert "$CA_CERTIFICATE" --fail --silent --max-time 2 \
    "$ENDPOINT/api/server/ping" >/dev/null 2>&1; then READY=1; break; fi
  sleep 1
done
[[ "$READY" == 1 ]] || { echo 'disposable HTTPS Immich did not become ready' >&2; exit 1; }

EMAIL="synthetic-$RUN_SUFFIX@example.invalid"
PASSWORD="synthetic-admin-$RUN_SUFFIX"
SIGNUP=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email:$email,password:$password,name:"Synthetic Disposable Admin"}')
curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$SIGNUP" "$ENDPOINT/api/auth/admin-sign-up" >/dev/null
LOGIN_REQUEST=$(jq -nc --arg email "$EMAIL" --arg password "$PASSWORD" '{email:$email,password:$password}')
LOGIN=$(curl --cacert "$CA_CERTIFICATE" --fail --silent --header 'Content-Type: application/json' \
  --request POST --data "$LOGIN_REQUEST" "$ENDPOINT/api/auth/login")
ACCESS_TOKEN=$(jq -er '.accessToken | select(type == "string" and length > 0)' <<<"$LOGIN")
KEY_RESPONSE=$(curl --cacert "$CA_CERTIFICATE" --fail --silent \
  --header 'Content-Type: application/json' --header "Authorization: Bearer $ACCESS_TOKEN" \
  --request POST --data '{"name":"immich-rs disposable production","permissions":["asset.upload","asset.read","server.about","user.read"]}' \
  "$ENDPOINT/api/api-keys")
API_KEY=$(jq -er '.secret | select(type == "string" and length > 0)' <<<"$KEY_RESPONSE")

echo 'production gate: TLS fault and cancellation matrix' >&2
FAULT_EVIDENCE=$(python3 "$REPOSITORY_ROOT/tests/integration/run_production_faults.py" \
  --binary "$BINARY" --listen-host "$TLS_HOST" --ca-certificate "$CA_CERTIFICATE" \
  --server-certificate "$SERVER_CERTIFICATE" --server-key "$SERVER_KEY" \
  --workspace "$WORKSPACE/faults")

SOURCE="$WORKSPACE/source"
CORPUS_MANIFEST="$WORKSPACE/corpus.json"
PLAN="$WORKSPACE/upload-plan.json"
CHECKPOINT="$WORKSPACE/checkpoint.sqlite"
DUPLICATE_CHECKPOINT="$WORKSPACE/duplicate.sqlite"
ARCHIVE_MANIFEST="$WORKSPACE/archive.json"
"$REPOSITORY_ROOT/scripts/materialize-phase2-corpus.sh" --image "$SERVER_IMAGE" \
  --source "$SOURCE" --manifest "$CORPUS_MANIFEST" --container "$GENERATOR" \
  --run-label "$RESOURCE_LABEL=$RUN_ID"

set +e
echo 'production gate: reject untrusted certificate' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" plan archive immich --server "$ENDPOINT" \
  --authorize-production-read >/dev/null 2>"$WORKSPACE/untrusted.stderr"
UNTRUSTED_EXIT=$?
set -e
[[ "$UNTRUSTED_EXIT" == 7 ]] || { echo 'untrusted certificate did not fail closed' >&2; exit 1; }

echo 'production gate: plan and inspect' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" plan upload folder --server "$ENDPOINT" \
  --authorize-production-read --ca-certificate "$CA_CERTIFICATE" \
  --label synthetic-production "$SOURCE" >"$PLAN"
INSPECTION=$("$BINARY" inspect upload-plan --plan "$PLAN")
PLAN_DIGEST=$(jq -er '.plan_sha256' <<<"$INSPECTION")
OPERATIONS=$(jq -er '.operations' <<<"$INSPECTION")
[[ "$OPERATIONS" == 4 ]]

echo 'production gate: offline dry-run' >&2
DRY_REPORT=$("$BINARY" apply upload --dry-run --plan "$PLAN" --source "$SOURCE" \
  --checkpoint "$CHECKPOINT" --authorize-production-write)
[[ ! -e "$CHECKPOINT" && $(jq -r '.would_upload' <<<"$DRY_REPORT") == 4 ]]

BACKUP_REFERENCE="synthetic-backup-$RUN_SUFFIX"
declare -a PRODUCTION_ARGUMENTS=(apply upload --server "$ENDPOINT" \
  --ca-certificate "$CA_CERTIFICATE" --plan "$PLAN" --source "$SOURCE" \
  --checkpoint "$CHECKPOINT" --authorize-production-read --authorize-production-write \
  --confirm-plan-sha256 "$PLAN_DIGEST" --expected-operations "$OPERATIONS" \
  --backup-reference "$BACKUP_REFERENCE")
set +e
echo 'production gate: reject mismatched confirmation' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${PRODUCTION_ARGUMENTS[@]/$PLAN_DIGEST/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}" \
  >/dev/null 2>"$WORKSPACE/mismatch.stderr"
MISMATCH_EXIT=$?
set -e
[[ "$MISMATCH_EXIT" == 2 && ! -e "$CHECKPOINT" ]]

echo 'production gate: apply, resume and duplicate convergence' >&2
FIRST_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${PRODUCTION_ARGUMENTS[@]}")
RESUME_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${PRODUCTION_ARGUMENTS[@]}")
PRODUCTION_ARGUMENTS[11]="$DUPLICATE_CHECKPOINT"
DUPLICATE_REPORT=$(IMMICH_RS_API_KEY="$API_KEY" "$BINARY" "${PRODUCTION_ARGUMENTS[@]}")
[[ $(jq -r '.created' <<<"$FIRST_REPORT") == 4 ]]
[[ $(jq -r '.resumed' <<<"$RESUME_REPORT") == 4 ]]
[[ $(jq -r '.duplicate' <<<"$DUPLICATE_REPORT") == 4 ]]

echo 'production gate: read-only archive plan' >&2
IMMICH_RS_API_KEY="$API_KEY" "$BINARY" plan archive immich --server "$ENDPOINT" \
  --authorize-production-read --ca-certificate "$CA_CERTIFICATE" >"$ARCHIVE_MANIFEST"
ARCHIVE_ASSETS=$(jq -er '.summary.assets' "$ARCHIVE_MANIFEST")
[[ "$ARCHIVE_ASSETS" == 4 ]] || {
  echo "production archive asset count was $ARCHIVE_ASSETS, expected 4" >&2
  exit 1
}
if grep -aF -- "$BACKUP_REFERENCE" "$CHECKPOINT" "$DUPLICATE_CHECKPOINT" >/dev/null; then
  echo 'raw backup reference entered a checkpoint' >&2
  exit 1
fi
if grep -aF -- "$API_KEY" "$PLAN" "$ARCHIVE_MANIFEST" "$CHECKPOINT" "$DUPLICATE_CHECKPOINT" >/dev/null; then
  echo 'API key entered an artifact' >&2
  exit 1
fi

STATISTICS=$(curl --cacert "$CA_CERTIFICATE" --fail --silent \
  --header "Authorization: Bearer $ACCESS_TOKEN" "$ENDPOINT/api/assets/statistics")
ASSET_COUNT=$(jq -er '.total' <<<"$STATISTICS")
[[ "$ASSET_COUNT" == 3 ]]
SERVER_VERSION=$(curl --cacert "$CA_CERTIFICATE" --fail --silent "$ENDPOINT/api/server/version")
CA_SHA256=$(sha256sum "$CA_CERTIFICATE" | cut -d' ' -f1)
BINARY_SHA256=$(sha256sum "$BINARY" | cut -d' ' -f1)

echo 'production gate: cleanup' >&2
cleanup
CLEANED=1
trap - EXIT HUP INT TERM
REMAINING_CONTAINERS=$(docker ps --all --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING_VOLUMES=$(docker volume ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
REMAINING_NETWORKS=$(docker network ls --quiet --filter "label=$RESOURCE_LABEL=$RUN_ID")
[[ -z "$REMAINING_CONTAINERS$REMAINING_VOLUMES$REMAINING_NETWORKS" ]]

jq -n --arg commit "$COMMIT_SHA" --arg binary_sha256 "$BINARY_SHA256" \
  --arg server_image "$SERVER_IMAGE" --arg valkey_image "$VALKEY_IMAGE" \
  --arg database_image "$DATABASE_IMAGE" --arg ca_sha256 "$CA_SHA256" \
  --argjson server_version "$SERVER_VERSION" --argjson dry_run "$DRY_REPORT" \
  --argjson first "$FIRST_REPORT" --argjson resume "$RESUME_REPORT" \
  --argjson duplicate "$DUPLICATE_REPORT" --argjson asset_count "$ASSET_COUNT" \
  --argjson archive_assets "$ARCHIVE_ASSETS" \
  --argjson fault_evidence "$FAULT_EVIDENCE" \
  --argjson cleanup_verified "$CLEANED" \
  '{schema:"phase7-disposable-production-v1",commit_sha:$commit,binary_sha256:$binary_sha256,images:{server:$server_image,valkey:$valkey_image,database:$database_image},tls:{custom_ca_sha256:$ca_sha256,hostname_verification:true,untrusted_certificate_exit:7,fault_matrix:$fault_evidence},authorization:{plan_digest_matched:true,operation_budget:4,backup_reference_hashed:true,mismatch_exit:2},server_version:$server_version,reports:{dry_run:$dry_run,first:$first,resume:$resume,duplicate:$duplicate},archive_assets:$archive_assets,asset_count:$asset_count,cleanup:{verified:($cleanup_verified == 1),labelled_containers:0,labelled_volumes:0,labelled_networks:0,private_keys_removed:true,credentials_removed:true}}' >"$OUTPUT"
