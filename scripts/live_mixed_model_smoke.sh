#!/usr/bin/env bash
set -euo pipefail

PORT="${PORT:-18080}"
AUTH_PATH="${AUTH_PATH:-$HOME/.codex/auth.json}"
UPSTREAM_BASE_URL="${UPSTREAM_BASE_URL:-https://chatgpt.com/backend-api}"
ANTHROPIC_UPSTREAM_BASE_URL="${ANTHROPIC_UPSTREAM_BASE_URL:-https://api.anthropic.com}"
ANTHROPIC_AUTH_TOKEN="${ANTHROPIC_AUTH_TOKEN:-${ANTHROPIC_UPSTREAM_API_KEY:-${ANTHROPIC_OAUTH_TOKEN:-}}}"
LOG="${LOG:-/tmp/codex-gw-mixed-model.log}"
OUT_DIR="${OUT_DIR:-/tmp/codex-gw-mixed-model}"
MODELS=(${MODELS:-gpt-5.4 claude-sonnet-4-6 gpt-5.4})

if [[ ! -f "$AUTH_PATH" ]]; then
  echo "missing Codex auth file: $AUTH_PATH" >&2
  exit 2
fi

for model in "${MODELS[@]}"; do
  case "$model" in
    claude-*|anthropic/claude-*)
      if [[ -z "$ANTHROPIC_AUTH_TOKEN" ]]; then
        echo "model '$model' will use local Claude Code credentials if available; otherwise set ANTHROPIC_OAUTH_TOKEN or ANTHROPIC_UPSTREAM_API_KEY" >&2
      fi
      ;;
  esac
done

mkdir -p "$OUT_DIR"
rm -f "$LOG" "$OUT_DIR"/*.json

cargo build -q --release

args=(
  ./target/release/codex-api-gateway
  --port "$PORT"
  --auth-path "$AUTH_PATH"
  --upstream-base-url "$UPSTREAM_BASE_URL"
  --anthropic-upstream-base-url "$ANTHROPIC_UPSTREAM_BASE_URL"
)
if [[ -n "$ANTHROPIC_AUTH_TOKEN" ]]; then
  args+=(--anthropic-auth-token "$ANTHROPIC_AUTH_TOKEN")
fi

CODEX_PROXY_VERBOSE="${CODEX_PROXY_VERBOSE:-1}" "${args[@]}" >"$LOG" 2>&1 &
GW_PID=$!
trap 'kill "$GW_PID" >/dev/null 2>&1 || true' EXIT

for _ in {1..50}; do
  if curl -fsS "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

curl -fsS "http://127.0.0.1:${PORT}/health" >/dev/null

idx=0
for model in "${MODELS[@]}"; do
  idx=$((idx + 1))
  marker="MIXED_MODEL_${idx}_OK"
  body="$OUT_DIR/request-${idx}.json"
  response="$OUT_DIR/response-${idx}.json"
  status_file="$OUT_DIR/status-${idx}.txt"

  python3 - "$model" "$marker" >"$body" <<'PY'
import json, sys
model, marker = sys.argv[1], sys.argv[2]
print(json.dumps({
    "model": model,
    "max_tokens": 64,
    "stream": False,
    "messages": [
        {"role": "user", "content": f"Reply with exactly: {marker}"}
    ],
}))
PY

  status="$(
    curl -sS -o "$response" -w '%{http_code}' \
      -X POST "http://127.0.0.1:${PORT}/v1/messages" \
      -H 'content-type: application/json' \
      -H 'anthropic-version: 2023-06-01' \
      -H 'authorization: Bearer local-proxy' \
      --data-binary "@$body"
  )"
  printf '%s\n' "$status" >"$status_file"

  if [[ "$status" != 2* ]]; then
    echo "request $idx model=$model failed with HTTP $status" >&2
    cat "$response" >&2
    exit 1
  fi

  if ! grep -q "$marker" "$response"; then
    echo "request $idx model=$model did not contain marker $marker" >&2
    cat "$response" >&2
    exit 1
  fi

  echo "ok request=$idx model=$model marker=$marker"
done

echo
echo "responses: $OUT_DIR"
echo "gateway log: $LOG"
echo
echo "routing evidence:"
grep -E '^\[anthropic-route\]|POST /v1/messages|/codex/responses|Anthropic upstream|anthropic-upstream-response-status|upstream-response-status|request-outcome|codex-events-summary' "$LOG" || true
