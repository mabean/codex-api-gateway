#!/usr/bin/env bash
set -euo pipefail

PORT="${PORT:-18084}"
AUTH_PATH="${AUTH_PATH:-$HOME/.codex/auth.json}"
UPSTREAM_BASE_URL="${UPSTREAM_BASE_URL:-https://chatgpt.com/backend-api}"
ANTHROPIC_UPSTREAM_BASE_URL="${ANTHROPIC_UPSTREAM_BASE_URL:-https://api.anthropic.com}"
LOG="${LOG:-/tmp/codex-gw-claude-code-acceptance.log}"
OUT_DIR="${OUT_DIR:-/tmp/codex-gw-claude-code-acceptance}"
WORKROOT="${WORKROOT:-/tmp/codex-gw-claude-code-work}"
MODELS=(${MODELS:-gpt-5.4 claude-sonnet-4-6})

if [[ ! -f "$AUTH_PATH" ]]; then
  echo "missing Codex auth file: $AUTH_PATH" >&2
  exit 2
fi

rm -rf "$OUT_DIR" "$WORKROOT"
mkdir -p "$OUT_DIR" "$WORKROOT"
rm -f "$LOG"

cargo build -q --release

args=(
  ./target/release/codex-api-gateway
  --port "$PORT"
  --auth-path "$AUTH_PATH"
  --upstream-base-url "$UPSTREAM_BASE_URL"
  --anthropic-upstream-base-url "$ANTHROPIC_UPSTREAM_BASE_URL"
)

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

safe_model_name() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9' '_'
}

run_claude() {
  local workdir="$1"
  local outfile="$2"
  shift 2
  (
    cd "$workdir"
    ANTHROPIC_BASE_URL="http://127.0.0.1:${PORT}" \
      ANTHROPIC_API_KEY="local-proxy" \
      CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
      "$@"
  ) >"$outfile" 2>&1
}

assert_file_contains() {
  local file="$1"
  local marker="$2"
  if ! grep -q "$marker" "$file"; then
    echo "missing marker '$marker' in $file" >&2
    tail -80 "$file" >&2 || true
    exit 1
  fi
}

assert_sse_text_contains() {
  local file="$1"
  local marker="$2"
  python3 - "$file" "$marker" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
marker = sys.argv[2]
text = []
for line in path.read_text(errors="ignore").splitlines():
    if not line.startswith("data:"):
        continue
    data = line[5:].strip()
    if not data or data == "[DONE]":
        continue
    try:
        event = json.loads(data)
    except json.JSONDecodeError:
        continue
    delta = event.get("delta")
    if isinstance(delta, dict) and isinstance(delta.get("text"), str):
        text.append(delta["text"])
    content = event.get("content")
    if isinstance(content, list):
        for block in content:
            if isinstance(block, dict) and isinstance(block.get("text"), str):
                text.append(block["text"])
assembled = "".join(text)
if marker not in assembled:
    print(f"missing marker {marker!r}; assembled={assembled!r}", file=sys.stderr)
    sys.exit(1)
PY
}

for model in "${MODELS[@]}"; do
  safe="$(safe_model_name "$model")"
  workdir="$WORKROOT/$safe"
  mkdir -p "$workdir"

  marker="${safe}_TEXT_OK"
  out="$OUT_DIR/${safe}-claude-text.out"
  run_claude "$workdir" "$out" \
    claude -p --output-format text --model "$model" \
    "Reply with exactly: $marker"
  assert_file_contains "$out" "$marker"
  echo "ok claude-text model=$model marker=$marker"

  marker="${safe}_FILE_TOOL_OK"
  out="$OUT_DIR/${safe}-claude-file-tool.jsonl"
  printf 'before\n' >"$workdir/note.txt"
  run_claude "$workdir" "$out" \
    claude -p --dangerously-skip-permissions --output-format stream-json --verbose \
      --model "$model" \
      "Use the Edit tool to replace the contents of note.txt with exactly $marker. After that, read the file and reply with its exact contents only."
  if [[ "$(cat "$workdir/note.txt")" != "$marker" ]]; then
    echo "file tool did not update note.txt for $model" >&2
    cat "$workdir/note.txt" >&2
    exit 1
  fi
  assert_file_contains "$out" "$marker"
  echo "ok claude-file-tool model=$model marker=$marker"

  marker="${safe}_BASH_TOOL_OK"
  out="$OUT_DIR/${safe}-claude-bash-tool.jsonl"
  run_claude "$workdir" "$out" \
    claude -p --dangerously-skip-permissions --output-format stream-json --verbose \
      --model "$model" \
      "Use the Bash tool to run: printf $marker . Then reply with exactly the command output and nothing else."
  assert_file_contains "$out" "$marker"
  echo "ok claude-bash-tool model=$model marker=$marker"

  marker="${safe}_DIRECT_JSON_OK"
  body="$OUT_DIR/${safe}-direct-json-request.json"
  response="$OUT_DIR/${safe}-direct-json-response.json"
  python3 - "$model" "$marker" >"$body" <<'PY'
import json
import sys

model, marker = sys.argv[1], sys.argv[2]
print(json.dumps({
    "model": model,
    "max_tokens": 64,
    "stream": False,
    "messages": [{"role": "user", "content": f"Reply with exactly: {marker}"}],
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
  if [[ "$status" != 2* ]]; then
    echo "direct json failed for $model with HTTP $status" >&2
    cat "$response" >&2
    exit 1
  fi
  assert_file_contains "$response" "$marker"
  echo "ok direct-json model=$model marker=$marker"

  marker="${safe}_DIRECT_SSE_OK"
  body="$OUT_DIR/${safe}-direct-sse-request.json"
  response="$OUT_DIR/${safe}-direct-sse-response.sse"
  python3 - "$model" "$marker" >"$body" <<'PY'
import json
import sys

model, marker = sys.argv[1], sys.argv[2]
print(json.dumps({
    "model": model,
    "max_tokens": 64,
    "stream": True,
    "messages": [{"role": "user", "content": f"Reply with exactly: {marker}"}],
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
  if [[ "$status" != 2* ]]; then
    echo "direct sse failed for $model with HTTP $status" >&2
    cat "$response" >&2
    exit 1
  fi
  assert_sse_text_contains "$response" "$marker"
  echo "ok direct-sse model=$model marker=$marker"
done

echo
echo "outputs: $OUT_DIR"
echo "workdirs: $WORKROOT"
echo "gateway log: $LOG"
echo
echo "routing/tool evidence:"
grep -E '^\[request\]|^\[anthropic-route\]|^\[anthropic-upstream-response-status\]|^\[responses-request\]|^\[upstream-response-status\]|^\[request-outcome\]|^\[codex-events-summary\]|^\[tool-path-stage\]|^\[anthropic-render' "$LOG" || true
