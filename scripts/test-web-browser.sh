#!/usr/bin/env bash
set -Eeuo pipefail

usage() {
  echo "Usage: scripts/test-web-browser.sh --binary PATH [--browser PATH]" >&2
  exit 2
}

binary=""
browser="${IMMICH_RS_TEST_BROWSER:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)
      [[ $# -ge 2 ]] || usage
      binary="$2"
      shift 2
      ;;
    --browser)
      [[ $# -ge 2 ]] || usage
      browser="$2"
      shift 2
      ;;
    *) usage ;;
  esac
done
[[ -n "$binary" && -x "$binary" ]] || usage
if [[ -z "$browser" ]]; then
  browser="$(command -v google-chrome || command -v chromium || true)"
fi
[[ -n "$browser" && -x "$browser" ]] || {
  echo "a Chrome-compatible browser is required" >&2
  exit 2
}

workspace_parent="${TMPDIR:-/tmp}"
workspace="$(mktemp -d "${workspace_parent%/}/immich-rs-web-browser.XXXXXX")"
server_pid=""
cleanup() {
  status=$?
  trap - EXIT INT TERM
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -INT "$server_pid" 2>/dev/null || true
    for _attempt in $(seq 1 100); do
      kill -0 "$server_pid" 2>/dev/null || break
      sleep 0.05
    done
    if kill -0 "$server_pid" 2>/dev/null; then
      kill -KILL "$server_pid" 2>/dev/null || true
    fi
    wait "$server_pid" 2>/dev/null || true
  fi
  case "$workspace" in
    "${workspace_parent%/}"/immich-rs-web-browser.*) rm -rf -- "$workspace" ;;
    *) echo "refusing unsafe browser workspace cleanup" >&2; status=1 ;;
  esac
  exit "$status"
}
trap cleanup EXIT INT TERM

mkdir -m 0700 "$workspace/source" "$workspace/state" "$workspace/profile"
printf '%s\n' 'synthetic-browser-bootstrap' >"$workspace/bootstrap.secret"
printf '%s\n' 'synthetic browser media' >"$workspace/source/photo.jpg"
chmod 0600 "$workspace/bootstrap.secret"
port="$(python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
    listener.bind(("127.0.0.1", 0))
    print(listener.getsockname()[1])
PY
)"
origin="http://127.0.0.1:$port"
cat >"$workspace/web.toml" <<EOF
schema_version = 1

[web]
listen_address = "127.0.0.1:$port"
public_origin = "$origin"
bootstrap_secret_file = "$workspace/bootstrap.secret"
history_state_id = "browser"

[web.limits]
accepted_connections = 2
header_read_seconds = 1

[[sources]]
id = "synthetic"
label = "Synthetic browser source"
allowed_root = "$workspace/source"
relative_root = "."
generation = 1

[[servers]]
id = "disposable"
origin = "http://127.0.0.1:9"
api_key_file = "$workspace/unread-api-key.secret"
mode = "disposable"
generation = 1
credential_generation = 1

[[states]]
id = "browser"
label = "Synthetic browser state"
allowed_root = "$workspace"
relative_root = "state"
generation = 1
EOF

"$binary" --config "$workspace/web.toml" >"$workspace/server.log" 2>&1 &
server_pid=$!
for _attempt in $(seq 1 100); do
  if curl --fail --silent --max-time 1 "$origin/pair" \
    >"$workspace/readiness.html"; then
    break
  fi
  kill -0 "$server_pid" 2>/dev/null || {
    echo "browser test server stopped before readiness" >&2
    exit 1
  }
  sleep 0.05
done
rg -q 'Pair this browser' "$workspace/readiness.html" || {
  echo "browser test server did not become ready" >&2
  exit 1
}

browser_flags=(
  --headless=new
  --disable-background-networking
  --disable-component-update
  --disable-default-apps
  --disable-sync
  --metrics-recording-only
  --no-default-browser-check
  --no-first-run
  "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"
  "--user-data-dir=$workspace/profile"
)
timeout 30 "$browser" "${browser_flags[@]}" --dump-dom "$origin/pair" \
  >"$workspace/browser.html" 2>"$workspace/browser.log"
timeout 30 "$browser" "${browser_flags[@]}" --window-size=1280,800 \
  "--screenshot=$workspace/pair.png" "$origin/pair" \
  >>"$workspace/browser.log" 2>&1

for marker in '<html lang="en">' 'class="skip-link"' 'id="main-content"' \
  'id="secret"' 'Pair this browser'; do
  rg -q "$marker" "$workspace/browser.html" || {
    echo "headless browser DOM omitted a required marker" >&2
    exit 1
  }
done
if rg -q 'synthetic-browser-bootstrap|unread-api-key|photo.jpg' \
  "$workspace/browser.html"; then
  echo "headless browser DOM exposed operator-only material" >&2
  exit 1
fi
python3 - "$workspace/pair.png" <<'PY'
import pathlib
import sys
if pathlib.Path(sys.argv[1]).read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
    raise SystemExit("headless browser screenshot is not a PNG")
PY
echo "Web Console headless browser regression passed: loopback DOM and screenshot"
