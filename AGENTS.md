# AGENTS.md — engineering rules for this repository

This repository builds a native desktop web browser in Rust. GPUI owns the
browser chrome; WebKit (`WKWebView`) owns page rendering. Read this file before
changing code.

## Non-negotiable invariants

1. **Real engine, real rendering.** Website content must come from a real
   browser engine. Never fake rendering with screenshots, HTML previews or a
   hand-written renderer.
2. **GPUI owns the chrome.** Tabs, toolbar, address bar, menus, settings and
   status indicators are GPUI elements. Do not replace browser chrome with
   AppKit controls.
3. **The UI never talks to WebKit types.** `browser-ui` and `browser-core`
   depend on `browser_engine::Engine`, never on `WKWebView`, `objc2`, or
   AppKit. Platform code stays inside `browser-engine` / `browser-platform`.
4. **Main thread only.** AppKit and WebKit objects are created and used on the
   main thread. Engine handles are `!Send`; do not move them across threads.
5. **No privileged page APIs.** Pages only talk to the browser through
   validated, one-way engine events. Never expose file, process, shell or
   native APIs to page JavaScript.
6. **Explicit errors.** No `let _ =` on fallible user-visible operations and no
   silent `unwrap()` in production paths. `Result` for operations that can
   fail, with logged context.
7. **Justify new dependencies.** Prefer the standard library and existing
   crates. Every new dependency needs a reason in the PR/commit description.
8. **No global mutable state.** State lives in `browser-core` models owned by
   GPUI entities, or is passed explicitly.

## Rust quality rules

- Stable Rust, edition 2024.
- Small modules and small functions. A file above ~600 lines needs a split
  proposal.
- Platform-specific code is isolated behind `cfg(target_os = "macos")` and the
  `Engine` trait. Non-macOS builds must compile and return
  `EngineError::UnsupportedPlatform`.
- `unsafe` blocks must carry a `// SAFETY:` comment explaining the invariant.
  Every `unsafe` use is a design decision, not a shortcut.
- Prefer `Rc`/`RefCell` on the UI thread over `Arc<Mutex>`; avoid locks in hot
  paths.
- No premature abstraction: add a trait when there is a second implementation
  or a clear test seam.

## Testing workflow (TDD)

For every vertical slice:

1. Write the acceptance criteria in the issue/commit or ROADMAP.md.
2. Write a failing unit/integration test in the appropriate crate.
3. Implement the minimum to pass.
4. Run `./scripts/cargo test --workspace`.
5. Refactor, then run tests again.
6. For UI or rendering changes, run the M0 probe and/or the manual checklist in
   `docs/testing/manual-checklist.md`.
7. Update ROADMAP.md and docs before declaring completion.

Tests must not depend on third-party websites. Use `browser-testserver`
(loopback HTTP fixtures). The M0 probe may use `https://example.com` as a real
HTTPS smoke check, but core tests must be deterministic.

## Quality gates (all must pass before a milestone is complete)

```sh
./scripts/cargo fmt --check
./scripts/cargo check --workspace
./scripts/cargo clippy --workspace --all-targets -- -D warnings
./scripts/cargo test --workspace
./scripts/m0-probe.sh          # engine embedding behavior
./scripts/m1-probe.sh          # browser window, tabs, navigation, typing
./scripts/m2-probe.sh          # session persistence across a real relaunch
./scripts/m3-probe.sh          # history, bookmarks, find, downloads, clear site data
```

`scripts/cargo` sets a workspace-local `CARGO_HOME`; use it.

## Repository layout

```
crates/
  browser-app       binary entry points + composition root
  browser-ui        GPUI chrome components, themes, text input
  browser-core      domain state: tabs, navigation, history, sessions
  browser-engine    Engine trait + platform adapters (WKWebView)
  browser-storage   SQLite persistence: history, bookmarks, session
  browser-security  URL policy, permissions, download policy
  browser-platform  OS integration: native view handle, shortcuts, dialogs
  browser-testserver deterministic HTTP fixtures for tests
docs/
  research/         ecosystem research
  decisions/        ADRs
  milestones/       milestone reports
  testing/          manual + automated test plan
scripts/            build/test/measure helpers
```

## Definition of "done"

Compiling is not done. A slice is done when it has tests, works in a real window
with a real web page, has been exercised manually where relevant, and is
documented. Do not claim Chromium-grade isolation or security properties WebKit
does not provide.

## Commit discipline

One coherent vertical slice per commit. Run the quality gates before committing.
Commit messages: `area: imperative summary` (for example
`engine: attach WKWebView and forward navigation events`).
