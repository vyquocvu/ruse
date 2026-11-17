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

## M2 — Tabs and sessions — **done**

Evidence: `docs/milestones/M2.md`; `m2-probe` (two processes: create session,
relaunch, verify order/active/lazy load); `m1-probe` favicon check.

- Tabs: create/close/switch/cycle/reorder (drag)/duplicate; per-tab busy dot.
- Favicons from page hints + `/favicon.ico` fallback, fetched off-thread.
- Session persistence (atomic JSON, ordered background writes) and restore.
- Lazy loading of restored background tabs.
- Web-content process termination shows a recoverable error page (not
  automatically triggerable through public APIs).
- History within a restored tab is not persisted (fresh engine history).

## M3 — Browser essentials (in progress)

| Task | Status | Acceptance |
| --- | --- | --- |
| M3.1 History: SQLite recording + GPUI history page + delete/clear | done | `m3-probe`: visits recorded, page lists them, delete/clear persist across reload |
| M3.2 Bookmarks + folders + manager page | next | add/remove current page, folders, open/delete entries; persisted |
| M3.3 Downloads (WKDownload, destination, progress, cancel) | next | fixture `/download` saves a file and appears in the list |
| M3.4 Find in page | next | Cmd+F finds text via WKWebView, next/previous, Esc closes |
| M3.5 Cookie management + clear browsing data (cache/storage) | next | clear actions remove WebKit records; private data policy documented |
| M3.6 Permissions prompts (camera/mic) via WKUIDelegate | next | prompt UI shown, denial/grant honored, decision remembered per origin |
| M3.7 Context menus | next | page context menu is native WebKit; chrome right-click actions for tabs/bookmarks |

History search in the UI, bookmark bar and download progress UI are refinements
that can land in M4 if they are not essential to the acceptance tests.

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
