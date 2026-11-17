#!/usr/bin/env bash
# Runs the M1 end-to-end browser probe (real GPUI window + real WebKit + real
# synthetic keyboard input) and prints the report summary.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BIN="${1:-target/debug/m1-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin m1-probe" >&2
  exit 2
fi
mkdir -p target
rm -f target/m1-report.json
LOG=target/m1-probe.log
"$BIN" >"$LOG" 2>&1
STATUS=$?
grep -E "^\[m1\] (PASS|FAIL|result)" "$LOG"
exit "$STATUS"
