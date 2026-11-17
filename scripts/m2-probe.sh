#!/usr/bin/env bash
# Runs the M2 session-restore probe as two real processes: phase1 creates tabs
# and a session file, phase2 relaunches and verifies restoration.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BIN="${1:-target/debug/m2-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin m2-probe" >&2
  exit 2
fi
mkdir -p target
rm -rf target/m2-data target/m2-report-phase1.json target/m2-report-phase2.json
"$BIN" --phase1 >target/m2-phase1.log 2>&1
P1=$?
if (( P1 != 0 )); then
  cat target/m2-phase1.log
  echo "phase1 failed" >&2
  exit 1
fi
"$BIN" --phase2 >target/m2-phase2.log 2>&1
P2=$?
cat target/m2-phase1.log target/m2-phase2.log
exit "$P2"
