# ADR-0002: Use WKWebView directly through objc2 as the v1 engine

Status: accepted (M0)

## Context

M0 required a real engine with navigation, JS, CSS, multiple views, input,
downloads, popups and permissions on macOS. Candidates: WKWebView directly,
`wry`, `gpui-webview`, WebView2, WebKitGTK. Details:
`docs/research/engine-integration.md`.

## Decision

Implement `browser-engine`'s macOS adapter with `WKWebView` via
`objc2-web-kit`, exposing an engine-agnostic `Engine` trait and
`EngineEvent` stream. Retain `WKNavigationDelegate`/`WKUIDelegate` ourselves.

## Rationale

- Full control over delegate policy: navigation allow/cancel/download, external
  schemes, `createWebView` popups, media permissions, process termination.
- Full API surface for later features: `pageZoom`, `findString`, cookies,
  website data store, `isInspectable` snapshots.
- No extra framework abstraction; roughly 700 lines of clearly owned FFI.
- Verified in M0: HTTPS rendering, JS, CSS, resize, two independent views,
  lifecycle isolation, snapshot capture.

## Consequences

- macOS compatibility equals Safari/WebKit, not Chromium. Documented.
- Windows/Linux require new adapters behind the same trait.
- We own delegate lifetime and teardown; covered by `close`/`Drop`.
- No Chromium-grade site-isolation claims are permitted.

## Rejected alternatives

- `wry`: good bootstrap, but its delegates own the behavior we must extend and
  it lacks find/zoom/download-progress control.
- `gpui-webview`: tied to the `gpui-kit` fork and experimental.
