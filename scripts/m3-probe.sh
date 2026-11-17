#!/usr/bin/env bash
# Runs the M3 history probe (real visits -> GPUI history page -> delete/clear ->
# reload from SQLite).
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BIN="${1:-target/debug/m3-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin m3-probe" >&2
  exit 2
fi
mkdir -p target
rm -rf target/m3-data target/m3-report.json
"$BIN" >target/m3-probe.log 2>&1
STATUS=$?
grep -E "^\[m3\]" target/m3-probe.log
exit "$STATUS"
