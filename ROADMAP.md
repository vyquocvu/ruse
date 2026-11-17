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

## M3 — Browser essentials — **done** (two items deferred)

Evidence: `docs/milestones/M3.md`; `m3-probe` (history, bookmarks/folders,
find-in-page, downloads, clear site data).

| Task | Status | Acceptance |
| --- | --- | --- |
| M3.1 History: SQLite recording + GPUI history page + delete/clear | done | verified by `m3-probe` across reloads |
| M3.2 Bookmarks + folders + manager page | done | add/remove, folders, move, delete; reload-verified |
| M3.3 Downloads (WKDownload, destination, progress, cancel) | done | fixture `/download` saved and listed; filename policy tested |
| M3.4 Find in page | done | match/no-match and close verified via native find |
| M3.5 Clear browsing data (cookies, caches, storage) | done | cookie + localStorage fixture cleared and re-verified |
| M3.6 Permission prompts (camera/mic) | deferred to M5 | requires parking WebKit decision blocks across a GPUI dialog; currently deny-by-default |
| M3.7 Chrome context menus / open link in new tab from page menu | deferred to M4 | page context menu is WebKit's native one today |
| M3.8 Copy URL, zoom controls | done | Cmd+Shift+C, Cmd+=/-/0 |

Remaining M3 refinements (history search UI, download resume, per-site cookie
manager) are tracked under M4/M5.

## M4 — Modern experience — **done** (with cosmetic deferrals)

Evidence: `docs/milestones/M4.md`, `m4-probe` (two processes: settings edits,
search behavior, homepage/theme restore).

- Light/dark/system themes with live OS appearance following.
- Settings page (homepage, search engine, theme, privacy actions) persisted.
- Command palette (Cmd+K) with commands, bookmarks, history and URL/search.
- New-tab page with recent sites and quick actions.
- Bookmark bar for unfiled bookmarks.
- Deferred: customizable toolbar, sidebar, pinned tabs, tab grouping, bookmark
  import/export, persisted download list.

## M5 — Privacy and security — **done** (permission prompts deferred)

Evidence: `docs/milestones/M5.md`, `m5-probe`, `SECURITY.md`, ADR-0007/0008.

- Private windows: per-window ephemeral store, no history/session, isolated from
  the persistent store and from each other.
- Clear cookies only, clear all site data, clear history.
- Popup rate limiting; external-scheme handoff; `javascript:` rejected.
- HTTPS/“Not secure” address-bar indicators.
- Security/trust-boundary documentation with explicit non-guarantees.
- Deferred: camera/microphone prompt UI (needs signed bundle + Info.plist),
  location/notifications (unsupported), per-site cookie inspector,
  dangerous-file warnings.

## M6 — Release hardening (next)

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
