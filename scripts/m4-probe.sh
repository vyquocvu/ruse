#!/usr/bin/env bash
# Runs the M4 settings/palette probe across two real processes.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BIN="${1:-target/debug/m4-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin m4-probe" >&2
  exit 2
fi
mkdir -p target
rm -rf target/m4-data target/m4-report-phase1.json target/m4-report-phase2.json
"$BIN" --phase1 >target/m4-phase1.log 2>&1
P1=$?
if (( P1 != 0 )); then
  cat target/m4-phase1.log
  echo "phase1 failed" >&2
  exit 1
fi
"$BIN" --phase2 >target/m4-phase2.log 2>&1
P2=$?
cat target/m4-phase1.log target/m4-phase2.log
exit "$P2"
