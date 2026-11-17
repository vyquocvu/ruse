# Security and privacy model

GPUI Browser is a native shell around Apple's WebKit (`WKWebView`). Web content
is untrusted; browser chrome and native integration are trusted. This document
states what the implementation actually guarantees and what it does not.

## Trust boundaries

- **Web content** runs in WebKit's web content processes. It renders into a
  native subview and has no access to native browser APIs.
- **Browser chrome** (GPUI) holds tabs, history, bookmarks, downloads, settings
  and the engine handles. It never executes page-provided code.
- **Native bridge**: there is currently **no JavaScript→native bridge**. Pages
  cannot call into the browser. The only script direction is native→page
  `evaluateJavaScript` for metadata such as the declared favicon URL; the
  result is parsed as data (JSON/URL), never executed natively.

## Navigation policy

- Rendered schemes: `http`, `https`, `about`, `data`, `blob`, `file`.
- Other schemes (`mailto:`, `tel:`, app schemes, ...) are cancelled in the
  WebKit navigation delegate and handed to `NSWorkspace` only after the user
  action reaches the browser; nothing is passed to a shell.
- `javascript:` typed in the address bar is never navigated; it is treated as a
  search query.
- `target="_blank"`, `window.open` and WebKit `createWebView` requests never
  create uncontrolled WebKit windows: the browser opens a tab it owns and
  applies a popup rate limit (5 requests / 3 seconds, unit-tested).
- External schemes are never loaded into a page.

## Cookies and site data

- Normal windows use WebKit's persistent default data store.
- **Private windows** each create one non-persistent `WKWebsiteDataStore` and
  share it across their own tabs. Closing the window discards cookies, caches
  and local storage. Private windows do not record history and are excluded
  from session persistence. Two private windows intentionally do **not** share
  cookies with each other (per-window isolation; stronger than Safari's shared
  private session).
- Clearing actions: clear cookies only; clear all site data (cookies, caches,
  local storage, IndexedDB, service workers, ...); clear browsing history.
  History/bookmarks/settings are stored in `~/Library/Application Support/GPUI Browser`
  (override: `GPUI_BROWSER_DATA_DIR`), never inside web storage.

## Downloads

- WebKit performs the transfer; the browser chooses the destination.
- Filenames are sanitised (`browser-security`, unit-tested): directory
  components and control/reserved characters are removed, leading dots hidden,
  length bounded, collisions resolved with ` (n)`.
- Destinations are forced inside the downloads directory
  (`~/Downloads`, override: `GPUI_BROWSER_DOWNLOADS_DIR`). Pages cannot choose a
  path.
- Files are never auto-opened. macOS quarantine applies to downloaded files.
- No resume, no authentication retry, no per-file progress callbacks (progress
  is sampled from the file size).

## Device permissions

- Camera and microphone requests are **denied by default** in the WebKit UI
  delegate. Granting them requires an application bundle with the appropriate
  `Info.plist` usage descriptions and a permission UI; that shipping work is
  scheduled with release packaging. Until then, media capture is unavailable.
- Location, notifications, clipboard-read and similar capabilities are not
  exposed by this implementation. Do not assume browser-grade permission
  prompts exist yet.

## What is NOT guaranteed

- **No Chromium-grade site isolation.** WebKit's process model is
  process-per-view; same-site frames may share a process. Cross-site isolation
  controls available in Chromium are not available here.
- No guarantee that WebKit's content-process sandbox is identical to Safari's
  configuration; the app uses the system WebKit framework's defaults.
- No extension isolation, enterprise policy, or certificate-pinning features.
- No protection against malicious files beyond OS quarantine and the download
  path policy.
- The app bundle is not yet sandboxed/signed for distribution; that is M6 work.

## Data inventory

| Data | Location | Private windows |
| --- | --- | --- |
| Cookies/cache/localStorage | WebKit data store | Ephemeral, discarded |
| History | `browser.sqlite3` | Not recorded |
| Bookmarks | `browser.sqlite3` | Shared (user data) |
| Settings | `settings.json` | Shared |
| Session | `session.json` | Not written |
| Downloads | `~/Downloads` | Same directory |
