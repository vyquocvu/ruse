# ADR-0003: Overlay strategy above web content

Status: accepted (M0), refining in Phase 1

## Context

The web view is a native subview above GPUI's layer. GPUI popovers, menus and
dialogs drawn in the same window are hidden behind page content. This is
inherent to native view embedding and also cited by `gpui-webview`'s docs.

## Decision

1. Anything that must appear above page content uses one of:
   - a full-page GPUI surface that **hides the web view** while open (settings,
     new tab page, command palette, site-info panel),
   - a native AppKit menu (`NSMenu`) for context and application menus,
   - a separate GPUI popup window positioned relative to the anchor.
2. Small GPUI toasts/status text live outside the page viewport (toolbar/sidebar)
   and never overlap it.
3. Never draw a GPUI overlay over the page and assume it is visible.

## Consequences

- Visual design accounts for a hard rectangular page area.
- Settings and new-tab pages are full-window GPUI views, which also avoids
  WebKit overhead for them.
- Menus feel native, which fits the product direction.

## Alternatives rejected

- Hiding the web view for every popover: causes page flicker; use native menus
  or popup windows instead.
