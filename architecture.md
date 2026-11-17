# Architecture

## Summary

GPUI Browser is a Rust workspace. GPUI renders the browser interface; an
embedded `WKWebView` (WebKit) renders website content as a native subview of the
GPUI window. The two halves communicate through a narrow, platform-independent
engine boundary.

```
┌──────────────────────────────────────────────────────────────────┐
│ browser-app (composition root, GPUI Application, window)         │
│                                                                  │
│  ┌────────────────────────┐      ┌─────────────────────────────┐ │
│  │ browser-ui             │      │ browser-core                │ │
│  │ tabs, toolbar, address │◄────►│ tab/navigation domain state │ │
│  │ bar, menus, settings   │      │ commands, events, history    │ │
│  └───────────┬────────────┘      └──────────────┬──────────────┘ │
│              │ Engine trait (Rc<dyn Engine>)    │                │
│  ┌───────────▼──────────────────────────────────▼──────────────┐ │
│  │ browser-engine: Engine trait, EngineEvent channel           │ │
│  │  └─ macos::WkEngine (WKWebView + delegates, !Send)          │ │
│  └───────────┬──────────────────────────────────────────────────┘ │
│              │ native NSView subview                             │
│  ┌───────────▼────────────┐   ┌───────────────┐                   │
│  │ GPUI window (AppKit)   │   │ browser-storage│ (SQLite, files)  │
│  │ chrome drawn by GPUI   │   │ browser-security│ (policy)       │
│  └────────────────────────┘   └───────────────┘                   │
└──────────────────────────────────────────────────────────────────┘
```

## Crates and responsibilities

| Crate | Responsibility | May depend on |
| --- | --- | --- |
| `browser-app` | Entry point, application lifecycle, window creation, composition of tabs/engine/UI, event pumps | all crates |
| `browser-ui` | GPUI components: tab strip, toolbar, address bar (text input), menus, dialogs, settings, themes | gpui, browser-core, browser-engine (trait only) |
| `browser-core` | Pure domain: tab ids/state, navigation state machine, URL vs search parsing, history/bookmark models, session model | std, serde, url |
| `browser-engine` | `Engine` trait, `EngineEvent`, macOS `WKWebView` adapter with delegates | objc2 family (macOS only), async-channel |
| `browser-storage` | SQLite history/bookmarks/settings, session files, file paths | rusqlite, serde, browser-core |
| `browser-security` | URL scheme policy, navigation policy, permission model, download filename policy | url, browser-core |
| `browser-platform` | AppKit integration: native view handle extraction, global shortcut monitor, OS dialogs, open external URLs | gpui, objc2 (macOS) |
| `browser-testserver` | Deterministic loopback HTTP fixtures for tests | std |

## Engine boundary

`browser-engine` exposes:

- `Engine` trait: navigation (`navigate`, `go_back`, `go_forward`, `reload`,
  `stop`), viewport/visibility, zoom, JS evaluation, page metadata, snapshots.
- `EngineEvent`: lifecycle (`NavigationStarted`, `NavigationCommitted`,
  `LoadFinished`, `LoadFailed`), metadata (`TitleChanged`, `UrlChanged`),
  policy requests (`NewWindowRequested`, `ExternalSchemeRequested`,
  `DownloadRequested`), `ProcessTerminated`, script results, snapshot results.
- `create_engine(EngineOptions) -> EngineInstance` containing an
  `Rc<dyn Engine>` and an `async_channel::Receiver<EngineEvent>`.

Events are unbounded-channel messages produced on the main thread inside
WebKit delegate callbacks. The app drains each tab's receiver in a GPUI task and
folds events into `browser-core` state. The engine never calls into GPUI and
never owns browser state. This keeps engine tests independent of the UI and
makes a second platform adapter possible without touching UI code.

## Threading model

- Everything UI, AppKit or WebKit related runs on the main thread.
- `Engine` handles are `!Send`; they are owned by GPUI entities.
- The event channel is the only cross-thread surface. The sender is held by
  delegates; the receiver is awaited on GPUI's main-thread executor.
- `browser-storage` performs blocking SQLite work; call sites must use
  `cx.background_executor().spawn(...)` and return results to the UI thread.
  Storage types are `Send`.

## Native view embedding

GPUI's macOS window returns an AppKit `NSView *` through
`raw_window_handle::HasWindowHandle` (the raw handle is the GPUI content view,
not the `NSWindow`). `browser-platform` extracts that pointer and wraps it in
`NativeParent`. `WkEngine`:

1. retains the parent view,
2. creates a `WKWebView` with a `WKWebViewConfiguration`,
3. adds it as a subview above GPUI's Metal-backed view,
4. positions it on every GPUI layout pass with `setFrame`.

GPUI coordinates (`Pixels`) are logical AppKit points, so the viewport maps
1:1 to points; the only transform is the vertical flip because NSView uses a
bottom-left origin while GPUI lays out from the top-left.

Consequences:

- Web content is a real native view: input, scrolling, text entry, video,
  file pickers, context menus and IME are handled by WebKit with native quality.
- GPUI-drawn content **cannot** paint above the web view. Popovers, menus and
  dialogs that must overlap page content use one of: hiding the web view for
  full-page surfaces (settings, new tab), AppKit native menus/alerts, or a
  separate GPUI popup window. This is a hard constraint of native view
  embedding, documented in ADR-0003.
- Web views must be hidden/destroyed before their window closes. The adapter
  removes itself from the superview on `close`/`Drop`.

## Tab model

One `WKWebView` per tab. WebKit spawns a `WebContent` XPC process per web view
(plus shared Networking/GPU helpers), so each tab has an independent JavaScript
context and DOM. Memory is inherently per-tab; M0 measured ~30 MB per simple
page's WebContent process plus shared helpers. Tabs are therefore not
"free" — the UI must make closing tabs easy, and future work should consider
hibernating inactive tabs after a timeout (documented in ROADMAP backlog, not
implemented).

## Navigation and state flow

1. UI action (Enter in the address bar, back button, page link) invokes an
   `Engine` method or a `browser-core` command.
2. WebKit loads; delegates emit `EngineEvent`s.
3. The app's event pump updates the owning tab's `browser-core` state
   (loading, URL, title, error) and calls `cx.notify()`.
4. GPUI re-renders chrome; the canvas element syncs the native viewport.
5. Security-relevant events (`NewWindowRequested`, `ExternalSchemeRequested`,
   `DownloadRequested`) pass through `browser-security` policy before the app
   acts on them.

The address bar is *not* overwritten while the user is editing it. URL updates
from the engine update the "current URL" model; the editing buffer only syncs
when it is not focused or when navigation completes without user edits.

## Persistence

`browser-storage` owns a single SQLite database under
`~/Library/Application Support/GPUI Browser/browser.sqlite3` (created on first
run) and atomic JSON for session state. Private sessions never write.

## Platform strategy

macOS is implemented and tested. `browser-engine` returns
`UnsupportedPlatform` elsewhere; the `Engine` trait is the seam for a future
WebView2 (Windows) or WebKitGTK (Linux) adapter. Do not add cross-platform
abstractions that are not backed by a working adapter.

## Known limitations (also see ADR-0002 / ADR-0003)

- GPUI overlays cannot render above the web view.
- Browser keyboard shortcuts do not reach GPUI while the web view is first
  responder; a local `NSEvent` monitor in `browser-platform` forwards them.
- WebKit provides process-per-view, not Chromium-grade site isolation; iframes
  from different origins may share a web content process. Do not claim
  otherwise.
- DevTools availability depends on `WKWebView.isInspectable` (macOS 13.3+).
- No favicon API exists in WKWebView; favicons require page hints or a
  network fetch.
