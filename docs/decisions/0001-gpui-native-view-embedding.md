# ADR-0001: Embed the web engine as a native AppKit subview of the GPUI window

Status: accepted (M0)

## Context

GPUI renders browser chrome into an AppKit `NSView` (`GPUIView`) that hosts a
Metal layer. No browser engine available on macOS can render a full web page
into a GPUI texture. The page must be rendered by WebKit while our chrome stays
GPUI-rendered.

GPUI's `Window` implements `raw_window_handle::HasWindowHandle`; on macOS the
handle is the GPUI content view. This gives a supported way to attach a native
sibling view.

## Decision

Attach the engine's `WKWebView` as a subview of GPUI's content view and position
it from GPUI layout. Convert GPUI's top-left coordinates to AppKit's bottom-left
origin using the parent view height on every layout pass. Keep the parent view
retained by the engine so teardown order is safe.

## Consequences

- Page rendering, input, IME, scrolling, video and context menus are native and
  correct without reimplementation.
- GPUI content can never paint above the web view (ADR-0003).
- The engine is macOS-specific behind the `Engine` trait; other platforms attach
  their own native views the same way.
- Window resize and DPI are handled by AppKit; only the frame needs syncing.

## Alternatives rejected

- Screenshot/OSR loop with WKWebView: no supported API, too slow, breaks input.
- Building page rendering in GPUI: explicitly out of scope.
