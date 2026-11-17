#!/usr/bin/env bash
# Runs the M0 feasibility probe and records peak RSS for the GPUI host process
# and for the WebKit helper processes it spawns. Browser engine memory is
# reported separately from GPUI overhead because they are different budgets.
#
# WebKit helper processes are XPC services, not direct children of the app, so
# they are identified as "WebKit processes that did not exist before launch".
# Run this on an idle machine (no Safari/other WebKit apps loading pages) for
# stable numbers.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN="${1:-target/debug/m0-probe}"
if [[ ! -x "$BIN" ]]; then
  echo "missing $BIN; run: ./scripts/cargo build -p browser-app --bin m0-probe" >&2
  exit 2
fi

mkdir -p target
rm -f target/m0-report.json target/m0-snapshot.png
LOG=target/m0-probe.log
SAMPLES=$(mktemp -t m0-samples)
BASELINE=$(mktemp -t m0-baseline)
trap 'rm -f "$SAMPLES" "$BASELINE"' EXIT

printf "0\n" >"$BASELINE"
ps -eo pid=,comm= | awk '/WebKit/ {print $1}' >>"$BASELINE"

"$BIN" >"$LOG" 2>&1 &
APP=$!
peak_app=0
peak_children=0

while kill -0 "$APP" 2>/dev/null; do
  ps -eo pid=,rss=,comm= >"$SAMPLES"
  app_rss=$(awk -v p="$APP" '$1==p {print $2}' "$SAMPLES")
  if [[ -n "${app_rss:-}" ]] && (( app_rss > peak_app )); then peak_app=$app_rss; fi

  children_total=$(awk 'NR==FNR { seen[$1]=1; next } /WebKit/ && !($1 in seen) { total += $2 } END { print total+0 }' "$BASELINE" "$SAMPLES")
  if (( children_total > peak_children )); then peak_children=$children_total; fi
  sleep 0.05
done

wait "$APP"
STATUS=$?

echo "exit_status=$STATUS"
echo "peak_gpui_process_mb=$(( (peak_app + 512) / 1024 ))"
echo "peak_webkit_helpers_mb=$(( (peak_children + 512) / 1024 ))"
echo "peak_total_mb=$(( (peak_app + peak_children + 512) / 1024 ))"
ps -eo pid=,rss=,comm= | awk 'NR==FNR { seen[$1]=1; next } /WebKit/ && !($1 in seen) { printf "%s %.0fMB %s\n", $1, $2/1024, $3 }' "$BASELINE" /dev/stdin </dev/null 2>/dev/null | head -10
BIN_BYTES=$(stat -f%z "$BIN" 2>/dev/null || stat -c%s "$BIN")
echo "probe_binary_mb=$(( (BIN_BYTES + 524288) / 1048576 ))"
grep -cE "^\[m0\] PASS" "$LOG" | sed 's/^/checks_passed=/'
grep -cE "^\[m0\] FAIL" "$LOG" | sed 's/^/checks_failed=/'
grep -E "^\[m0\] result" "$LOG"
exit "$STATUS"
