# Roadmap

Single source of truth for milestone status. Update after each verified slice.
Status values: `done`, `in progress`, `next`, `blocked`, `backlog`.

## M0 — Technical feasibility — **done**

Acceptance: real GPUI window with a real web engine, HTTPS navigation, JS, CSS,
input, resize, multiple views, memory measurement, platform limitations, engine
ADR.

Evidence: `docs/milestones/M0.md`, `target/m0-report.json`, `m0-probe`.

## M1 — Browser foundation (Phase 1)

Goal: a real browser window a user can navigate with, using GPUI chrome.

| Task | Status | Acceptance |
| --- | --- | --- |
| M1.1 `browser-core` URL/search classification + navigation state machine (TDD) | next | unit tests for URL vs query, about:, hosts, failures, loading states |
| M1.2 `browser-ui` theme + reusable components (button, icon button, address bar shell) | next | components render in window, consistent spacing/colors |
| M1.3 GPUI single-line text input (focus, typing, selection, paste, IME-safe) | next | typing in address bar works; Enter navigates; Esc reverts |
| M1.4 Tab strip + navigation toolbar wired to one live tab | next | back/forward/reload/stop act on the web view and track state |
| M1.5 Address bar ↔ engine navigation, loading progress, title/URL updates | next | title and URL update; progress visible; no flicker while editing |
| M1.6 Error page for failed navigation | next | failed host shows GPUI error with retry |
| M1.7 Keyboard shortcuts while page focused (local NSEvent monitor) | next | Cmd+L/T/W/R/[ ]/F work with page focused |
| M1.8 Homepage + search engine settings model | next | URL bar search uses configured engine |
| M1.9 M1 verification: probe extension + manual checklist | next | quality gates pass; probe navigates via app actions |

## M2 — Tabs and sessions

- Create/close/switch/reorder (drag)/duplicate; independent contexts.
- New tab page (GPUI, hides page view).
- Tab titles, favicons (fetch/hint), loading indicators, pinned state later.
- Session persistence + restore; recently closed tabs; crash recovery UX.

## M3 — Browser essentials

- History (SQLite, searchable), bookmarks + folders, bookmark bar.
- Downloads: WKDownload destination selection, progress, cancel, open/reveal.
- Search engine + homepage settings; zoom controls; find-in-page.
- Copy URL; open link in new tab (policy from `NewWindowRequested`).
- Context menus (native NSMenu for page actions), permissions prompts,
  cookie management, clear browsing data.

## M4 — Modern experience

- Light/dark themes with OS appearance; customizable toolbar; sidebar.
- Pinned tabs, tab grouping, command palette, quick search.
- Recent sites, custom new tab page, settings UI, bookmark import/export.

## M5 — Privacy and security

- Private windows with non-persistent data store; ephemeral session.
- Cookie controls, permission prompts (location/camera/mic/notifications),
  popup policy, download restrictions, external URL safety, HTTPS indicators.
- Clear site data; per-site data isolation documentation; bridge hardening.
- Document WebKit's real security boundaries.

## M6 — Release hardening

- Performance baselines: cold/warm start, idle CPU, memory 1/5/10 tabs,
  navigation latency, build/install size.
- Test expansion, `cargo clippy -D warnings`, CI config.
- App bundle (Info.plist, icon, signing notes), crash reporting guidance.
- Limit: do not ship claims of sandbox isolation or Chromium compatibility.

## Backlog (not scheduled)

- Tab hibernation after inactivity.
- Downloads: resume, progress UI.
- Custom user agent per site; reader mode.
- Session restore with lazy loading.
- Windows/Linux engine adapters.
- CEF off-screen renderer as an optional build.
- PDF viewer / print.
