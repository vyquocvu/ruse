# GPUI Browser

A native desktop web browser written in Rust. Browser chrome is built with
[GPUI](https://github.com/zed-industries/zed) (the Zed UI framework); websites
are rendered by WebKit (`WKWebView`) as a real native view inside the window.

macOS is the first-class platform. The engine boundary is designed for future
Windows (WebView2) and Linux (WebKitGTK) adapters, but only macOS is implemented
and validated today.

## Status

Milestone **M0 (technical feasibility)** is complete. The WKWebView embedding,
navigation events, JavaScript execution, CSS rendering, resize handling,
multiple independent web views, snapshots and memory measurement are verified.
See `docs/milestones/M0.md`.

The full browser interface (tabs, address bar, history, bookmarks, settings,
downloads, privacy) is under active development — see `ROADMAP.md`.

## Requirements

- macOS 13+ (validated on macOS 27.2.1, Apple Silicon)
- Xcode command-line tools (Metal shader compilation)
- Stable Rust (edition 2024; validated with 1.97.1)

## Build and run

```sh
./scripts/cargo build -p browser-app --bin gpui-browser
./scripts/cargo run -p browser-app --bin gpui-browser
```

`scripts/cargo` keeps the Cargo registry in the workspace (`.cargo-home`) so
builds also work in restricted environments.

## Verify the engine integration

```sh
./scripts/cargo build -p browser-app --bin m0-probe
./scripts/m0-probe.sh
cat target/m0-report.json
```

The probe opens a real window, loads a real HTTPS site plus deterministic local
fixtures, checks JavaScript/CSS/resize/multi-view behavior and writes a page
snapshot to `target/m0-snapshot.png`.

## Quality gates

```sh
./scripts/cargo fmt --check
./scripts/cargo check --workspace
./scripts/cargo clippy --workspace --all-targets -- -D warnings
./scripts/cargo test --workspace
```

## Documentation

- `AGENTS.md` — engineering rules
- `architecture.md` — system architecture
- `product-spec.md` — product requirements
- `tdd.md` — testing strategy
- `ROADMAP.md` — milestones and status
- `docs/research/engine-integration.md` — engine/UI research
- `docs/decisions/` — ADRs (embedding, engine choice, overlays, tabs)
- `docs/milestones/M0.md` — feasibility report

## Known limitations

- GPUI cannot draw overlays above the native web view; the UI uses full-page GPUI
  surfaces that hide the page, native AppKit menus, or popup windows (ADR-0003).
- Browser keyboard shortcuts require an `NSEvent` monitor while WebKit owns
  focus (implemented in Phase 1).
- WebKit is not Chromium: no Chromium-grade site isolation or extension
  compatibility is claimed or implied.
- Favicons and find-in-page are being built on top of WebKit primitives.
