# Performance baseline and methodology

Measured with `scripts/perf.sh` on 2026-11-17, macOS 27.2.1, Apple Silicon, release
profile (`lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`).

## Method

- `perf-probe` seeds a deterministic local homepage and a fixed-port fixture
  server, then measures:
  - **startup**: from the start of GPUI's `Application::run` (app construction,
    settings load, window creation) to the first completed page load,
  - **navigation**: average of five warm local navigations between fixture
    pages,
  - **idle CPU**: host-process CPU time delta over three idle seconds after a
    1.5 s settle,
  - **memory**: host RSS plus the RSS of WebKit helper processes started after
    the app, sampled after all tabs settle,
  - **tab open time**: time to open and load the remaining tabs.
- Cold = first run with an empty data directory. Warm = second run reusing the
  HTTP cache/cookies.
- Browser engine memory is reported separately from host (GPUI + WebKit UI
  process) memory.

## Results

| Run | Tabs | Startup | Navigation | Host | WebKit helpers | Total | Idle CPU |
| --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 1 | 375 ms | 46 ms | 111 MB | 64 MB | 175 MB | 1.0% |
| warm | 1 | 404 ms | 41 ms | 111 MB | 64 MB | 175 MB | 0.7% |
| warm | 5 | 402 ms | 38 ms | 107 MB | 180 MB | 287 MB | 1.3% |
| warm | 10 | 414 ms | 32 ms | 108 MB | 314 MB | 422 MB | 1.7% |

- Release binary: 11.7 MB. `.app` bundle: 12.7 MB (includes a generated icon).
- Release build (incremental warm): ~24 s; from scratch ~2 min on this host.
- Memory grows ~29 MB per additional tab (one WebKit WebContent process per
  view plus its share of the GPU/networking helpers). This is engine behavior,
  not GPUI overhead; tab hibernation is backlog.
- Startup numbers include app/window construction and the first page load.
- Idle CPU is the host process only; WebKit helpers idle separately. Polling
  backs off to 1.2 s when idle and metadata IPC to every fourth idle tick.

## What was optimized

- One engine per tab; no reload on tab switch.
- Adaptive ticker: 80 ms while loading/downloading, 1.2 s idle, metadata polled
  every fourth idle tick.
- Background threads for icons and storage; ordered background writers for
  session/settings; SQLite opened per call to avoid long-lived locks.
- Lazy session restore: background tabs are not navigated until activated.
- Release LTO and symbol stripping.

## Not optimized yet

- Inactive tabs keep full WebKit processes alive (no hibernation).
- Startup loads settings/session synchronously (small files; measured fast).
- No shared process pooling or prewarming.
