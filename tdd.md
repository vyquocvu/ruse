# Testing strategy (TDD)

## Rules

- Tests are written before implementation for every vertical slice.
- Tests never depend on third-party websites. Deterministic pages come from
  `browser-testserver` over `127.0.0.1`.
- Engine and window behavior is verified with the M0 probe, which drives a real
  GPUI window and real `WKWebView`s and writes `target/m0-report.json`.
- A milestone is not complete until the automated suites and the relevant
  manual checklist pass.

## Test pyramid

### Unit tests (`browser-core`, `browser-security`, `browser-storage`)

Fast, deterministic, no GUI:

- URL vs search input classification, trimming, punycode/host detection,
  `about:` and `file:` handling.
- Navigation state machine: start/commit/finish/fail transitions, stalled
  loads, back/forward availability, stop.
- Tab model: create/close/activate/reorder/duplicate, active index rules,
  last-tab behavior, private-tab segregation, closed-tab stack.
- Session serialization: round-trip, ordering, active tab restore, corrupt
  session fallback.
- History/bookmark storage: insert, query, dedupe, delete, folders, clear.
- Security policy: scheme allow/deny lists, external scheme handoff, download
  filename sanitization, popup policy, permission decision matrix.

### Engine adapter tests (`browser-engine`)

Deterministic pieces without a window:

- Event enum and viewport math (top-left to bottom-left conversion).
- URL parsing errors.
- Delegate policy classification helpers.

Live engine behavior lives in the M0 probe and future engine integration
binaries, because WKWebView requires a main-thread AppKit application.

### Integration / end-to-end tests (`browser-app`, `browser-testserver`)

The `m0-probe` binary is the current E2E harness. It asserts:

- HTTPS navigation and title extraction (`https://example.com`).
- Local page: JavaScript execution (inline and external), CSS application,
  form presence, page globals.
- Snapshot written to disk (visual evidence).
- Resize: page `innerWidth` follows the window.
- Two web views: independent JavaScript contexts, lifecycle isolation.
- Memory: `scripts/m0-probe.sh` samples host RSS and WebKit helper RSS
  separately.

Future slices extend the probe (or add sibling probes) with:

- tab create/close/switch through actual actions,
- back/forward/reload/stop round-trips,
- keyboard shortcut injection via synthesized `NSEvent`s,
- history/bookmarks persistence across restarts,
- private session storage isolation,
- download flow against `/download`.

### Manual verification (`docs/testing/manual-checklist.md`)

Needed for anything visual or input-related that cannot be automated reliably:
typing, IME, trackpad scrolling, keyboard focus handoff, external links, video,
file uploads, DevTools.

## Test fixtures

`browser-testserver` routes:

| Path | Purpose |
| --- | --- |
| `/` | HTML + CSS + JS + form + link fixture |
| `/css` | stylesheet used by `/` |
| `/js` | external script used by `/` |
| `/page2` | second page for navigation/back-forward |
| `/redirect` | 302 redirect |
| `/download` | `Content-Disposition: attachment` |
| `/slow` | 800 ms delay, for stop/loading-indicator tests |
| unknown | 404 page |

The server binds `127.0.0.1:0` (random port) and is always shut down with the
test.

## Commands

```sh
./scripts/cargo test --workspace
./scripts/cargo clippy --workspace --all-targets -- -D warnings
./scripts/m0-probe.sh
```

## Quality gates

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- M0 probe passes when the engine/window changed

## What "verified" means

A feature is verified when:

1. an automated test exercises its logic, and
2. it has been observed working in a real window (probe or manual checklist),
3. its failure modes are handled (errors surfaced, no panic, no silent drop).

Screenshots or JSON facts from the probe are stored under `target/`; they are
evidence, not a substitute for tests.
