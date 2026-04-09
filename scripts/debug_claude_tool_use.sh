#!/usr/bin/env bash
set -euo pipefail

PORT="${PORT:-18080}"
AUTH_PATH="${AUTH_PATH:-/Users/max/.openclaw/agents/main/agent/auth-profiles.json}"
UPSTREAM_BASE_URL="${UPSTREAM_BASE_URL:-https://chatgpt.com/backend-api}"
WORKDIR="${WORKDIR:-/tmp/codex-gw-test}"
LOG="${LOG:-/tmp/codex-gw-run.log}"
OUT="${OUT:-/tmp/claude-acceptance.out}"
PROMPT="${PROMPT:-Use the Edit tool to replace the contents of note.txt with exactly TOOL_USE_OK. After that, read the file and reply with its exact contents only.}"

pkill -f "codex-api-gateway --port ${PORT}" >/dev/null 2>&1 || true
pkill -f "/Users/max/Dev/openclaw/codex-openai-proxy/target/release/codex-api-gateway" >/dev/null 2>&1 || true
sleep 1

mkdir -p "$WORKDIR"
printf 'before\n' > "$WORKDIR/note.txt"
rm -f "$LOG" "$OUT"

cargo build -q --release
CODEX_PROXY_VERBOSE="${CODEX_PROXY_VERBOSE:-1}" ./target/release/codex-api-gateway \
  --port "$PORT" \
  --auth-path "$AUTH_PATH" \
  --upstream-base-url "$UPSTREAM_BASE_URL" \
  > "$LOG" 2>&1 &
GW_PID=$!
trap 'kill "$GW_PID" >/dev/null 2>&1 || true' EXIT
sleep 2

(
  cd "$WORKDIR"
  ANTHROPIC_BASE_URL="http://127.0.0.1:${PORT}" \
  ANTHROPIC_API_KEY="local-proxy" \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  python3 - <<'PY'
import os, subprocess, signal
out = open(os.environ['OUT'], 'w')
cmd = [
    'claude','-p','--dangerously-skip-permissions','--output-format','stream-json','--verbose',
    '--model','claude-sonnet-4-5', os.environ['PROMPT']
]
p = subprocess.Popen(cmd, stdout=out, stderr=subprocess.STDOUT, env=os.environ.copy())
try:
    p.wait(timeout=25)
except subprocess.TimeoutExpired:
    p.send_signal(signal.SIGTERM)
    try:
        p.wait(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill(); p.wait()
print(p.returncode)
PY
) >/tmp/codex-gw-exit.txt 2>&1

echo '=== CLAUDE OUTPUT ==='
cat "$OUT" 2>/dev/null || true
echo
echo '=== NOTE CONTENTS ==='
cat "$WORKDIR/note.txt" 2>/dev/null || true
echo
echo '=== HTTP REQUEST COUNT ==='
grep -c 'POST /v1/messages' "$LOG" || true
echo
echo '=== DEBUG MARKERS ==='
grep -E '^\[anthropic-ingress\]|^\[responses-request\]|^\[responses-request-tools\]|^\[upstream-request-body-bytes\]|^\[upstream-request-body-prefix\]|^\[upstream-response-status\]|^\[upstream-response-headers\]|^\[upstream-response-body-bytes\]|^\[upstream-response-body-prefix\]|^\[upstream-response-body\]|^\[upstream-read-error\]|^\[codex-parse-error\]|^\[request-outcome\]|^\[tool-path-stage\]|^\[codex-events-summary\]|^\[anthropic-render-summary\]|^\[anthropic-render-stop\]|^\[anthropic-wire-summary\]|^\[raw-codex-sse-begin\]|^\[raw-codex-sse-end\]|^\[raw-anthropic-sse-begin\]|^\[raw-anthropic-sse-end\]' "$LOG" || true
echo
echo
echo '=== FIRST TURN CLASSIFICATION ==='
LOG_PATH="$LOG" python3 - <<'PY2'
import os, re, pathlib
log = pathlib.Path(os.environ['LOG_PATH']).read_text(errors='ignore')
stages = re.findall(r'^\[tool-path-stage\] (.+)$', log, flags=re.M)
first = None
for s in stages:
    if s in ('codex_tool_path_detected', 'codex_no_tool_path'):
        first = s
        break
print(first or 'unknown')
PY2
echo '=== GATEWAY LOG TAIL ==='
tail -200 "$LOG" || true
