#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q --release
exec python3 scripts/live_gateway_checks.py --suite protocol "$@"
