# Roadmap

Single source of truth for milestone status. Update after each verified slice.
Status values: `done`, `in progress`, `next`, `blocked`, `backlog`.

## M0 — Technical feasibility — **done**

Acceptance: real GPUI window with a real web engine, HTTPS navigation, JS, CSS,
input, resize, multiple views, memory measurement, platform limitations, engine
ADR.

Evidence: `docs/milestones/M0.md`, `target/m0-report.json`, `m0-probe`.

## M1 — Browser foundation (Phase 1) — **done**

Goal: a real browser window a user can navigate with, using GPUI chrome.

Evidence: `docs/milestones/M1.md` and `target/m1-report.json` (15/15 probe
checks, including real synthetic keyboard input into chrome and page).

| Task | Status | Acceptance |
| --- | --- | --- |
| M1.1 `browser-core` URL/search classification + navigation state machine (TDD) | done | 31 unit tests: URL vs query, dangerous schemes, hosts, failures, loading states, tabs |
| M1.2 `browser-ui` theme + reusable components (button, icon button, tabs, loading bar) | done | themed components used by the app chrome |
| M1.3 GPUI single-line text input (focus, typing, selection, paste, IME) | done | `EntityInputHandler` implementation; automated typing + Enter |
| M1.4 Tab strip + navigation toolbar wired to live tabs | done | back/forward/reload-stop act on the engine; tabs create/close/switch |
| M1.5 Address bar ↔ engine navigation, loading progress, title/URL updates | done | title/URL/progress update; user edits never overwritten |
| M1.6 Error page for failed navigation | done | failed load shows GPUI error with retry; recovery verified |
| M1.7 Keyboard shortcuts while page focused (local NSEvent monitor) | done (manual check for real key events) | monitor forwards Cmd+L/T/W/R/[/], zoom, tab cycling |
| M1.8 Homepage + search engine settings model | done | `BrowserSettings` with tested defaults and sanitisation |
| M1.9 M1 verification: probe extension + manual checklist | done | `scripts/m1-probe.sh`, clippy/tests green |

## M2 — Tabs and sessions (next)

- Reorder tabs by drag; duplicate tab UI affordance.
- Rich new tab page (GPUI, hides page view) with recent sites/search.
- Favicons (page hint / fetch) and per-tab loading indicators.
- Session persistence + restore across restarts; session file format tests.
- Crash recovery UX for `webViewWebContentProcessDidTerminate` (retry/reload).
- Pin tabs mentally prepared but UI in M4.

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
