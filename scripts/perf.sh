#!/usr/bin/env bash
# Release-build performance baseline: startup, navigation latency, idle CPU and
# memory for 1/5/10 tabs. Run on an otherwise idle machine for stable numbers.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
mkdir -p target/perf
BUILD_START=$(python3 -c 'import time; print(time.time())')
"$ROOT/scripts/cargo" build --release -p browser-app --bin perf-probe --bin gpui-browser >/dev/null
BUILD_END=$(python3 -c 'import time; print(time.time())')
echo "release_build_seconds=$(python3 -c "print(round($BUILD_END-$BUILD_START,1))")"
echo "binary_size_kb=$(( $(stat -f%z target/release/gpui-browser) / 1024 ))"
if [[ -d "target/package/GPUI Browser.app" ]]; then
  echo "bundle_size_kb=$(du -sk "target/package/GPUI Browser.app" | awk '{print $1}')"
fi

rm -rf target/perf-data
run_variant() {
  local name="$1" tabs="$2"
  target/release/perf-probe --tabs "$tabs" >"target/perf/$name.log" 2>&1 || {
    echo "perf run $name failed" >&2
    tail -20 "target/perf/$name.log" >&2
    exit 1
  }
  cp target/perf-report.json "target/perf/$name.json"
}
run_variant cold_1 1
run_variant warm_1 1
run_variant tabs_5 5
run_variant tabs_10 10

python3 - <<'PY'
import json, pathlib
rows = []
for name in ["cold_1","warm_1","tabs_5","tabs_10"]:
    path = pathlib.Path("target/perf") / f"{name}.json"
    data = json.loads(path.read_text())
    rows.append((name, data))
print(f"{'run':<8} {'tabs':>4} {'startup':>9} {'nav':>7} {'hostMB':>7} {'webkitMB':>9} {'totalMB':>8} {'idleCPU%':>8}")
for name, d in rows:
    print(f"{name:<8} {d['tabs']:>4} {d['startup_ms']:>8.0f}ms {d['navigation_ms']:>6.0f}ms "
          f"{d['host_rss_mb']:>7.1f} {d['webkit_rss_mb']:>9.1f} {d['total_rss_mb']:>8.1f} "
          f"{d['idle_cpu_percent']:>7.2f}")
PY
