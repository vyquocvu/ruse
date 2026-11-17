# GPUI Browser

A native desktop web browser written in Rust. Browser chrome is built with
[GPUI](https://github.com/zed-industries/zed) (the Zed UI framework); websites
are rendered by WebKit (`WKWebView`) as a real native view inside the window.

macOS is the first-class platform. The engine boundary is designed for future
Windows (WebView2) and Linux (WebKitGTK) adapters, but only macOS is implemented
and validated today.

## Status

Milestones **M0 (engine feasibility)**, **M1 (browser foundation)**,
**M2 (tabs and sessions)**, **M3 (browser essentials)** and
**M4 (modern experience)** and **M5 (privacy and security)** are complete:

- Real native window with GPUI chrome: tab strip, toolbar, address bar, loading
  indicator, error page and a GPUI new-tab page.
- Real WebKit page rendering with navigation, back/forward, reload/stop,
  JavaScript, CSS, forms and keyboard input.
- Tabs (create/close/switch), independent page contexts and history per tab.
- Address bar accepts URLs or search queries; real typing and Enter are covered
  by an automated end-to-end probe.
- Browser shortcuts while the page focused are forwarded by a local NSEvent
  monitor (manual verification for real key events).
- Drag-and-drop tab reordering, duplicate tab, per-tab loading indicators.
- Favicons discovered from page hints and fetched off the UI thread.
- Session persistence with atomic writes and restore; background tabs are
  lazy-loaded on activation.
- History (SQLite + GPUI page), bookmarks with folders, downloads manager,
  find-in-page, clear site data, copy URL and zoom.
- Settings page (homepage, search engine, theme), command palette, new-tab page
  with recent sites, bookmark bar, light/dark themes.
- Private windows with ephemeral storage, no history or session leakage.
- Clear cookies/site data/history, popup rate limiting, HTTPS indicators.
- Verified evidence: `docs/milestones/M0.md` … `M5.md`; see `SECURITY.md` for
  the trust model and explicit non-guarantees.

Release hardening (performance baselines, app bundle, permission prompts) is
next — see `ROADMAP.md`.

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

## Verify the integration

```sh
./scripts/cargo build -p browser-app --bin m0-probe --bin m1-probe
./scripts/m0-probe.sh        # engine embedding: HTTPS, JS, CSS, resize, 2 views
./scripts/m1-probe.sh        # browser window: tabs, navigation, typing, favicon
./scripts/m2-probe.sh        # session persistence across two processes
./scripts/m3-probe.sh        # history, bookmarks, find, downloads, clear data
./scripts/m4-probe.sh        # settings persistence + palette across restarts
./scripts/m5-probe.sh        # private windows, store isolation, no leakage
cat target/m0-report.json target/m1-report.json target/m2-report-phase2.json target/m3-report.json target/m5-report.json
```

The M0 probe writes a page snapshot to `target/m0-snapshot.png`. The M1 probe
drives the real browser window with synthetic AppKit key events.

## Default shortcuts

| Shortcut | Action |
| --- | --- |
| ⌘L | Focus address bar, select all |
| ⏎ | Navigate to the typed URL / search |
| ⌘[ / ⌘] | Back / forward |
| ⌘R / ⇧⌘R | Reload / reload bypassing cache |
| ⌘T / ⌘W | New tab / close tab |
| ⇧⌘T | Reopen closed tab |
| ⇧⌘D | Duplicate tab |
| ⌘Y | History |
| ⇧⌘B | Bookmarks |
| ⇧⌘J | Downloads |
| ⌘K | Command palette |
| ⌘F / ⌘G / ⇧⌘G | Find / next / previous |
| ⌘D | Bookmark current page |
| ⇧⌘C | Copy current URL |
| ⇧⌘N | New private window |
| ⌃Tab / ⌃⇧Tab | Next / previous tab |
| ⌘= / ⌘- / ⌘0 | Zoom in / out / reset |
| ⌘Q | Quit |

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
- `docs/milestones/M1.md` — browser foundation report
- `docs/milestones/M2.md` — tabs and sessions report
- `docs/milestones/M3.md` — browser essentials report
- `docs/milestones/M4.md` — modern experience report
- `docs/milestones/M5.md` — privacy and security report
- `SECURITY.md` — threat model, guarantees and non-guarantees

## Known limitations

- GPUI cannot draw overlays above the native web view; the UI uses full-page GPUI
  surfaces that hide the page, native AppKit menus, or popup windows (ADR-0003).
- Browser keyboard shortcuts require a local `NSEvent` monitor while WebKit owns
  focus; the monitor is implemented, but real-key interception is on the manual
  checklist (`docs/testing/manual-checklist.md`).
- WebKit is not Chromium: no Chromium-grade site isolation or extension
  compatibility is claimed or implied (see SECURITY.md).
- Camera/microphone access is denied by default until signed packaging adds
  permission prompts.
- Favicons and find-in-page are being built on top of WebKit primitives.
