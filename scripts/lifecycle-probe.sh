#!/usr/bin/env bash
# Verifies Cmd+Q and last-tab-close both terminate the application.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BIN="${1:-target/debug/lifecycle-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin lifecycle-probe" >&2
  exit 2
fi
mkdir -p target
STATUS=0
for mode in quit close-tab; do
  START=$(python3 -c 'import time; print(time.time())')
  "$BIN" "--$mode" >"target/lifecycle-$mode.log" 2>&1
  CODE=$?
  END=$(python3 -c 'import time; print(time.time())')
  ELAPSED=$(python3 -c "print(round($END-$START,2))")
  if (( CODE == 0 )); then
    echo "[lifecycle] $mode: PASS (${ELAPSED}s)"
  else
    echo "[lifecycle] $mode: FAIL exit=$CODE (${ELAPSED}s)"
    tail -5 "target/lifecycle-$mode.log"
    STATUS=1
  fi
done
exit "$STATUS"
