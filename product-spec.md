# GPUI Browser — product specification

## Vision

A fast, quiet, native desktop browser. Rust-first, GPUI chrome, real WebKit page
rendering. macOS is the first-class platform; the architecture keeps a portable
engine boundary for Windows/Linux later.

## Principles

- Native, not Electron-like: real AppKit window, real web engine, no bundled
  web-app shell.
- Minimal chrome: one compact toolbar, tabs, unobtrusive status.
- Privacy by default: private mode with ephemeral storage, explicit permission
  prompts, cookied/storage controls, clear-data tools.
- Keyboard-first: every core action has a shortcut.
- Honest limits: document what WebKit does and does not guarantee.

## Primary users

Developers and power users on macOS who want a lightweight browser with native
performance, keyboard navigation and sensible privacy defaults.

## Phase 1 — working browser foundation

The application must launch as a native GPUI window, render real sites, support
URL/search input, back/forward/reload/stop, loading state, title and URL
display, navigation errors, and keyboard shortcuts. The UI contains a tab strip,
navigation toolbar, address bar, web content area and loading indicator.

Acceptance:

- HTTPS sites render with CSS and JavaScript; links and forms work; scrolling
  and text entry work inside pages.
- The address bar accepts URLs and search queries; Enter navigates.
- Back/forward/reload/stop work and their enabled state tracks the page.
- Page title and URL update as navigation changes.
- A failed navigation shows an error state, not a blank window.
- Core shortcuts work even while a page has keyboard focus.

## Phase 2 — tabs and sessions

Create/close/switch/reorder/duplicate tabs, new-tab page, independent page
contexts, tab titles and favicons, loading indicators, session save/restore,
recently closed tabs, crash recovery.

Acceptance: every tab has its own engine instance; closing a tab releases it;
restarting restores the session; a crashed web process does not crash the
browser.

## Phase 3 — browser essentials

History, bookmarks with folders, downloads, search engine/homepage settings,
zoom, find-in-page, copy URL, open link in new tab, context menus, permissions,
cookie management, clear browsing data.

## Phase 4 — modern experience

Light/dark themes following the OS, bookmark bar, sidebar, pinned tabs, command
palette, quick search, recent sites, custom new-tab page, settings UI,
bookmark import/export.

## Phase 5 — privacy and security

Private sessions with non-persistent storage, cookie policy, permission prompts
(location, camera, microphone, notifications), popup policy, download
restrictions, safe external URL handling, HTTPS indicators, per-site data
clearing, strict bridge rules and navigation policy.

## Non-goals for v1

- Implementing an HTML/CSS/JS engine.
- Chrome extension compatibility.
- Chromium-grade site isolation or sandbox guarantees.
- Cross-platform engine parity before macOS is solid.
- Sync services or accounts.

## Behaviour details

- **URL vs search**: input containing a scheme (`https:`, `about:`, `file:`)
  navigates; input that parses as a hostname navigates as HTTPS; everything else
  becomes a search query with the configured engine (default DuckDuckGo).
- **New windows**: `target="_blank"` links and permitted popups open as new
  tabs in the same window, never as uncontrolled WebKit windows.
- **External schemes**: `mailto:`, `tel:` and app schemes are never loaded into
  a page; they are handed to the OS after user-visible policy checks.
- **Private windows**: use `WKWebsiteDataStore.nonPersistentDataStore`; closing
  the last private tab discards the session.
- **Downloads**: saved to `~/Downloads` unless configured; filenames are
  sanitized and collisions resolved; downloads never auto-execute.
