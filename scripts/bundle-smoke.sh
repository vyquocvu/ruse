#!/usr/bin/env bash
# Smoke test for the packaged app: Info.plist validity, code signature, real
# launch with an isolated data directory, WebKit helper processes, clean exit.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
APP="target/package/GPUI Browser.app"
BIN="$APP/Contents/MacOS/gpui-browser"

if [[ ! -x "$BIN" ]]; then
  echo "missing bundle; run ./scripts/package.sh" >&2
  exit 2
fi
plutil -lint "$APP/Contents/Info.plist" >/dev/null || { echo "invalid Info.plist" >&2; exit 1; }
codesign --verify "$APP" >/dev/null 2>&1 || { echo "code signature invalid" >&2; exit 1; }

DATA=target/bundle-smoke-data
rm -rf "$DATA"
GPUI_BROWSER_DATA_DIR="$DATA" GPUI_BROWSER_DOWNLOADS_DIR=target/bundle-smoke-downloads \
  "$BIN" >target/bundle-smoke.log 2>&1 &
PID=$!
sleep 4
if ! kill -0 "$PID" 2>/dev/null; then
  echo "bundle process exited early" >&2
  tail -10 target/bundle-smoke.log >&2
  exit 1
fi
HELPERS=$(ps -eo comm | grep -c "WebKit.WebContent")
if (( HELPERS < 1 )); then
  echo "no WebKit content process spawned" >&2
  kill "$PID" 2>/dev/null
  exit 1
fi
kill "$PID" 2>/dev/null
wait "$PID" 2>/dev/null
echo "bundle smoke: PASS (process + $HELPERS WebKit content process(es), plist + signature valid)"
