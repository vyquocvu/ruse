# Engine integration research (M0)

Date: 2026-11-17. Host: macOS 27.2.1, Apple Silicon, Rust 1.97.1, GPUI 0.2.2.

This document records what was verified from source and by prototype, not from
README claims. The working prototype is `m0-probe` plus `browser-engine`'s
`macos` module.

## UI framework: GPUI 0.2.2 (crates.io, published 2025-10-22)

Verified from the published crate source:

- Entry point: `Application::new().run(|cx: &mut App| ...)`;
  `cx.open_window(WindowOptions { window_bounds, titlebar, .. }, |window, cx| Entity)`.
- `Window` implements `raw_window_handle::HasWindowHandle` and
  `HasDisplayHandle`. On macOS the AppKit handle points at the **GPUI content
  view** (`GPUIView`), not the `NSWindow`. That is exactly what is needed to
  attach a native subview.
- `Pixels` are logical points on macOS: `content_size()` returns AppKit points
  as `Pixels` with no scale multiplication. GPU scale is applied inside the
  renderer. A WKWebView frame can therefore use GPUI bounds directly (with a
  vertical flip, because NSView's origin is bottom-left).
- Layout coordinates received by `canvas`'s paint callback are window-relative
  points with a top-left origin. The prototype positions the web view from those
  bounds and verified page `innerWidth` tracks window resizes exactly.
- `Application::run` can `open_window`, `cx.activate`, `window.resize`,
  `cx.quit`; `window.spawn(cx, |&mut AsyncWindowContext|)` is available without
  the test-support feature. `gpui::smol_timeout` is test-only.
- GPUI 0.2.2 internally uses the old `objc`/`cocoa` crates; `objc2` 0.6 can be
  used alongside it (verified: the prototype builds and runs). We define
  uniquely named classes to avoid collisions.
- GPUI renders into its content view's layer. Native subviews added to that
  view composite **above** all GPUI-drawn content (see ADR-0003).
- The published GPUI has no screenshot/render-to-image API for the chrome, and
  screen recording permission is unavailable in the dev harness, so chrome
  verification is behavioural; page rendering is verified with
  `WKWebView.takeSnapshot`.

## Candidate integrations

### 1. `wry` 0.57.2 (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux)

Verified from wry source:

- `WebViewBuilder::build_as_child` supports child web views on macOS, Windows
  and Linux/X11 and accepts any `HasWindowHandle`. GPUI's window satisfies it.
- Provides navigation handler, document-title handler, `evaluate_script`,
  cookies, user agent, initialization scripts, IPC, devtools, download
  started/completed handlers, incognito.
- Missing higher-level browser needs: find-in-page, page zoom, per-appearance
  overlays above the web view, full popup policy, download progress, and
  fine-grained `WKNavigationDelegate` control. Wry owns the native delegates;
  extending them means reaching through to the WKWebView anyway.
- Container abstraction (`Rect`, logical sizes) adds a layer that a
  browser-grade app would outgrow.

**Verdict: viable bootstrap, rejected as foundation.** It would save ~500 lines
of FFI now and cost control over the delegate callbacks the browser features
(permissions, downloads, popups, crash recovery) depend on.

### 2. `gpui-webview` 0.10.0 / `gpui-kit`

- Depends on `gpui-kit` 0.10 (a separate GPUI distribution) and a forked
  `lb-wry` 0.53.3. Version-locked to that GPUI line, not the crates.io `gpui`
  used here.
- README itself calls it experimental; overlay support is limited to a
  `gpui-fast` backend and has documented fallback behaviour.
- The crate solves exactly the overlay problem we must solve long-term, which
  is useful evidence that the problem is real, but it cannot be combined with
  GPUI 0.2.2 without switching the entire UI stack.

**Verdict: rejected.** Wrong GPUI line; experimental.

### 3. Direct `WKWebView` via `objc2-web-kit` 0.3.2

- Typed bindings for `WKWebView`, `WKNavigationDelegate`, `WKUIDelegate`,
  `WKScriptMessageHandler`, `WKSnapshotConfiguration`, cookie store, website
  data store, preferences, find-in-page and `isInspectable`.
- Full control of navigation policy (allow/cancel/download), external schemes,
  new-window requests, media capture permission requests, web process crash
  callbacks, JS dialogs, snapshots and page zoom.
- Native input, IME, scrolling, video, file upload and context menus come free
  because the page is a real AppKit view.
- Cost: ~700 lines of FFI/delegate code and manual lifecycle management.
- macOS only; Windows/Linux need separate adapters behind the same `Engine`
  trait.

**Verdict: selected.** Prototype verified HTTPS loading, JS execution, CSS,
resize, two independent views, lifecycle isolation, snapshots.

### 4. WebView2 / WebKitGTK

- Windows-only / Linux-only respectively. Cannot serve macOS-first validation.
  They remain the natural adapters for the `Engine` trait later.

### 5. WKWebView off-screen rendering

- WKWebView has no supported off-screen/GPU-texture API. `takeSnapshot` is
  asynchronous, viewport-limited and too slow for interactive frames. A
  screenshot loop is not a browser.

**Verdict: rejected.** Native view embedding is the supported approach.

## Feature matrix (macOS, verified or documented)

| Capability | Direct WKWebView | wry |
| --- | --- | --- |
| Real HTML/CSS/JS | WebKit | WebKit |
| Native view embedding in GPUI | yes (verified) | yes (child webview) |
| Keyboard/mouse/IME in page | native | native |
| Multiple independent views | yes (verified) | yes |
| Navigation events | full delegate | handler subset |
| Title updates | delegate + JS bridge | handler |
| JS execution | `evaluateJavaScript` | yes |
| Find in page | `findString` | no |
| Page zoom | `pageZoom` | no dedicated API |
| Downloads | `decidePolicy` + `WKDownload` | started/completed |
| Popup policy | `createWebView` | partial |
| Permission prompts | `WKUIDelegate` | partial |
| DevTools | `isInspectable` (13.3+) | devtools feature |
| Favicon | no API (page hint/fetch) | no |
| GPUI overlays above page | no (native subview) | no |
| Process model | per-view WebContent | per-view |
| Bundle cost | OS WebKit | OS WebKit |

## Risks and mitigations

1. **Overlays cannot cover the page.** Mitigate with full-page GPUI surfaces
   that hide the native view, AppKit-native menus, and popup windows. ADR-0003.
2. **Shortcuts while page focused.** WebKit is first responder; GPUI never sees
   keys. Mitigate with a local `NSEvent` monitor in `browser-platform` that
   forwards browser shortcuts and swallows only those it handles.
3. **Memory per tab.** Each view spawns a WebContent process. Mitigate by
   explicit tab closing, later tab hibernation, and honest measurement.
4. **WebKit is Safari's engine.** Compatibility is WebKit/Safari compatibility,
   not Chromium. Document per-site issues honestly.
5. **No site isolation claims.** WebKit shares web processes across same-site
   frames and does not expose Chromium-style site isolation controls.
6. **App lifecycle.** Web views must be torn down before their window; the
   adapter removes itself from the superview on close/drop.
