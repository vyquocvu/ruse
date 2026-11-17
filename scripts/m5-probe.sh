#!/usr/bin/env bash
# Runs the M5 private-browsing probe: three windows, ephemeral stores, and
# history/session leakage checks.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BIN="${1:-target/debug/m5-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin m5-probe" >&2
  exit 2
fi
mkdir -p target
rm -rf target/m5-data target/m5-downloads target/m5-report.json
"$BIN" >target/m5-probe.log 2>&1
STATUS=$?
grep -E "^\[m5\]" target/m5-probe.log
exit "$STATUS"
