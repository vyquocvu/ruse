# wry vs Servo for a GPUI browser

Compared 2026-11-17. wry 0.57.2, `servo` crate 0.8.0 (released 2026-11-07).

Both are Rust crates that put web content in a native app, but they are
different categories:

- **wry is a WebView wrapper, not a browser engine.** It embeds the platform's
  engine — WKWebView on macOS/iOS, WebView2 on Windows, WebKitGTK on Linux —
  and exposes a cross-platform Rust API around it.
- **Servo is a browser engine.** The `servo` crate links the whole engine
  (layout, CSS, SpiderMonkey JS, WebRender, networking, storage) into your
  process and hands you a `WebView` plus a rendering context to paint.

So "wry vs Servo" is really "system WebView vs a second engine that you host and
compose yourself". For this project, which already embeds WKWebView directly,
wry is the closest alternative; Servo is a different engine strategy.

## Summary

| Dimension | wry 0.57.2 | servo 0.8.0 |
| --- | --- | --- |
| What you embed | OS WebView (Safari/Edge/WebKitGTK) | Servo engine + OpenGL renderer |
| Web compatibility | Whatever the OS browser has | Servo's current web-platform coverage |
| Consistency across OS | Different engine per OS | Same engine everywhere it builds |
| GPUI integration | Native child view (same model as WKWebView) | You own painting/compositing and input forwarding |
| Rendering | OS compositor/GPU | OpenGL (GL/GLES) via surfman; software fallback |
| Input/IME/scrolling | Native | Forwarded by the embedder; IME UI via delegate |
| Features you inherit | Video, DRM, PDF, context menus, native text input | Servo's feature set; no download or find API exposed |
| Control over engine | Limited by wry's delegates | Full engine API and events |
| Maturity | Production-infrastructure (Tauri) | Pre-1.0 prototype engine, monthly breaking releases |
| Build cost | Uses system frameworks; small | Huge dependency graph; long builds; SpiderMonkey/WebRender |
| Binary/runtime footprint | Small (no engine shipped) | Large (engine, resources, JS runtime) |
| License | Apache-2.0 OR MIT | MPL-2.0 |
| Security model | Inherits platform engine sandboxing/cadence | Multiprocess; optional sandbox unsupported on Apple Silicon |

## Where wry wins

1. **It is the shortest path to a working embedded browser.** Native child views
   handle mouse, keyboard, IME, scrolling, selection, video, file uploads and
   context menus. The M0 work in this repo proved the same native-subview model
   for WKWebView; wry source confirms `build_as_child` takes any
   `HasWindowHandle`, which GPUI's window provides.
2. **Web compatibility is the platform browser's.** On macOS that is Safari's
   WebKit; on Windows it is Edge/Chromium. Sites, video, PDFs and DRM work the
   way users expect on each OS.
3. **Tiny build and distribution cost.** No engine is compiled or shipped; the
   app links against system frameworks. This repo's GPUI + direct WKWebView
   bundle is 12.7 MB; a wry build would be in the same range.
4. **Mature operational story.** wry is the WebView layer behind Tauri and has
   explicit support for child webviews, IPC, navigation/title callbacks,
   cookies, devtools, downloads and incognito.

wry's costs (from the M0 research): you do not get find-in-page, page zoom,
download progress or fine-grained `WKNavigationDelegate` policy through its API.
It owns the native delegates, so permission/popup/crash behavior must be worked
around or forked. GPUI overlays still cannot paint above the native view.

## Where Servo wins

1. **One engine everywhere.** Behavior is not split across WebKit, WebView2 and
   WebKitGTK. For embedded HTML in a cross-platform app, that consistency is
   valuable.
2. **Rust-native, memory-safe engine.** No system WebView dependency, no
   per-OS engine differences, and the whole stack is inspectable and hackable.
3. **A real embedding API with breadth.** The 0.8 crate exposes navigation and
   history, title/URL/favicon/status, JavaScript evaluation, user scripts, page
   and pinch zoom, screenshots, accessibility (AccessKit), theme changes,
   input events, context menus, dialogs, file pickers, permission requests,
   cookies/site data, devtools and optional WebGL/WebGPU/WebXR/gamepad/bluetooth.
4. **Offscreen and software rendering options.** `OffscreenRenderingContext`
   and `SoftwareRenderingContext` return RGBA images, which makes it possible to
   composite Servo output into another UI toolkit.
5. **Parallel engine architecture.** Servo's layout/style engine is built around
   parallelism and Rust safety; this is the long-term bet of the project.

## Why wry is still the practical choice for this browser

The integration cost of Servo in GPUI is the deciding factor, not its feature
list.

- Servo's `WindowRenderingContext` creates an **OpenGL** surface for a raw
  window handle and, by default, **paints the entire window surface**. GPUI
  renders its chrome with Metal into its own layer. Handing Servo GPUI's window
  means two renderers owning the same surface, and Servo's window context would
  paint over the browser chrome. The documented way to paint only a region is an
  `OffscreenRenderingContext`, which is still an OpenGL surface that must be
  read back or shared into GPUI.
- The practical GPUI options are:
  1. **SoftwareRenderingContext + readback** into a GPUI texture. Servo's own
     docs warn this is "generally bad performance"; acceptable for static
     panels, questionable for scrolling/video.
  2. **Offscreen OpenGL + texture sharing/readback**, with a custom frame
     clock, resize handling and input forwarding.
  3. **A dedicated child NSView with its own OpenGL context** (the native-view
     pattern, but now Servo drives the GL surface). This might avoid readback,
     but it is unverified and still requires forwarding every input event to
     `WebView::notify_input_event`.
- Servo expects the embedder to forward keyboard, mouse, wheel, touch, focus and
  IME events, draw context menus/dialogs/file pickers, and drive
  `Servo::spin_event_loop` from the host event loop. wry gives all of that from
  the OS engine for free.
- Browser essentials are not all exposed: the 0.8 embedding API has no download
  manager and no find-in-page API; there is no PDF/DRM/WebRTC integration in the
  crate. The engine is self-described as a "prototype web browser engine" and
  still publishes monthly compatibility improvements; that is not yet a
  daily-driver compatibility target.
- Security: Servo defaults to multiprocess, and has an optional gaol sandbox,
  but the source explicitly says sandboxing is unsupported on Windows, iOS,
  ARM/RISC-V targets and Android — **including Apple Silicon macOS**. WebKit's
  content-process sandbox is a much more mature baseline for a browser.
- Build/distribution: the Servo crate locks ~1,009 packages, needs Xcode plus
  SpiderMonkey/WebRender build tooling, and docs.rs' average successful build is
  ~15m44s (0.8.0 itself failed to build on docs.rs). This is a heavy addition to
  a project that currently builds against system frameworks.
- License: MPL-2.0 (file-level copyleft) versus wry's Apache-2.0/MIT, a
  consideration if the app is distributed under different terms.

## Recommendation

- **For this repository's goal — a daily-usable GPUI browser on macOS — keep the
  direct WKWebView engine.** It gives the same native integration wry offers,
  with finer control over delegates (downloads, permissions, popups, crashes)
  and without a wrapper's abstraction. wry would be the pragmatic choice if we
  did not already own that integration.
- **Servo is not a drop-in alternative today.** Its engine breadth is
  promising, but GPUI compositing, input/IME plumbing, missing browser APIs and
  pre-1.0 compatibility make it a research spike, not a production engine for
  this release.
- **If Servo is explored later**, do it as a separate `Engine` adapter behind
  the existing trait, starting with a measured spike:
  1. `SoftwareRenderingContext` + GPUI texture at a fixed size; measure frame
     latency, CPU and memory on a real page.
  2. Then test a dedicated child view with an OpenGL `WindowRenderingContext`
     and full input/IME forwarding.
  3. Only then decide whether to invest in an offscreen GPU path and browser
     feature parity.

## Evidence and caveats

- Servo facts come from the `servo` 0.8.0 crate source (embedding docs in
  `lib.rs`, `WebView`/`WebViewDelegate` API, `servo-paint-api` 0.8.0 rendering
  contexts, crate features and lockfile) and Servo's public documentation.
  Servo was **not built or prototyped** here; build-time and binary-size claims
  are limited to what the package metadata and docs.rs report.
- wry facts come from the 0.57.2 crate source inspected in M0
  (`docs/research/engine-integration.md`) plus the M0 prototype of the same
  native-view embedding model. wry itself was not run in this project.
- No performance comparison between Servo and WKWebView was measured here; the
  repo's performance numbers are for GPUI + WKWebView only.

## References

- Servo crate: <https://crates.io/crates/servo> ·
  <https://docs.rs/crate/servo/0.8.0>
- Servo embedding API docs: <https://github.com/servo/servo/blob/main/components/servo/lib.rs>
- Servo 0.1.0 release (LTS, pre-1.0, monthly breaking changes):
  <https://servo.org/blog/2026/04/13/servo-0.1.0-release/>
- Servo WPT tracking: <https://servo.org/wpt/>
- Servo September 2026 embedding changes:
  <https://servo.org/blog/2026/11/05/september-in-servo/>
- wry: <https://crates.io/crates/wry> · <https://github.com/tauri-apps/wry>
- Local wry/engine research: [engine-integration.md](engine-integration.md)
